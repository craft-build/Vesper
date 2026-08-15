//! Interactive SAS verification driver (checkpoint 08).
//!
//! One session at a time: [`Session`] owns the SDK `VerificationRequest` plus
//! an action channel the UI writes through commands, and pumps SDK state
//! changes into the App-scope `ClientState::verification` signal (written
//! from the runtime thread — legal because the signal uses `SyncStorage` and
//! is created in the App scope; see `api::ClientState` docs).
//!
//! Flow (SAS-only, no QR):
//! 1. `request_verification_with_methods([SasV1])` on the device or user
//!    identity → publish `Requested`.
//! 2. When the other side accepts, the request transitions to `Ready`; we
//!    then `accept()` + `start_sas()` and start listening on the
//!    `SasVerification` change stream.
//! 3. `KeysExchanged` → publish the 7-emoji short-auth string
//!    (`EmojisShown`); `Confirm` action → `.confirm()` → `Confirmed`;
//!    `Mismatch` → `.mismatch()`; either side's `Done`/`Cancelled` ends the
//!    session.
//!
//! Everything here runs inside the runtime thread's tokio tasks; nothing
//! non-`Send` crosses the UI seam (the signal payloads are plain data).

use dioxus_signals::{ReadableExt, WritableExt};
use futures::StreamExt;
use matrix_sdk::{
    encryption::{
        identities::{Device, UserIdentity},
        verification::{SasVerification, VerificationRequest},
    },
    ruma::{events::key::verification::VerificationMethod, OwnedDeviceId, OwnedUserId, UserId},
    Client,
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

pub(crate) fn unknown_error(msg: String) -> matrix_sdk::Error {
    matrix_sdk::Error::UnknownError(anyhow::anyhow!(msg).into())
}

use crate::{
    api::ClientState,
    model::{SasEmoji, VerificationSession, VerificationState, VerificationTarget},
};

/// Publish a new state for the active session into the bound UI signal.
/// `state` replaces the previous state; emojis are replaced only when
/// `Some` is passed (they stay visible through `Confirmed`).
fn publish(state: &ClientState, new_state: VerificationState, emojis: Option<Vec<SasEmoji>>) {
    let mut verification = state.verification;
    let current = verification.read().clone();
    let session = current.map(|mut s| {
        s.state = new_state;
        if let Some(emojis) = emojis {
            s.emojis = emojis;
        }
        s
    });
    // Replace-or-clear: `set` only when different so readers with
    // value-equality guards don't spin (checkpoint-06 effect lesson).
    if verification.read().clone() != session {
        verification.set(session);
    }
}

fn emojis_from(sas: &SasVerification) -> Option<Vec<SasEmoji>> {
    sas.emoji().map(|list| {
        list.iter()
            .map(|e| SasEmoji {
                symbol: e.symbol.to_string(),
                description: e.description.to_string(),
            })
            .collect()
    })
}

/// Map the SDK's `SasState` into the UI state machine.
fn map_sas_state(sas: &SasVerification) -> Option<VerificationState> {
    use matrix_sdk::encryption::verification::SasState::*;
    Some(match sas.state() {
        Created { .. } | Started { .. } | Accepted { .. } => VerificationState::Ready,
        KeysExchanged { .. } => {
            if sas.supports_emoji() {
                VerificationState::EmojisShown
            } else {
                // Decimal-only SAS: we don't render decimals; treat as
                // unsupported rather than a dead-end dialog.
                VerificationState::Failed("This session doesn't support emoji verification.".into())
            }
        }
        Confirmed => VerificationState::Confirmed,
        Done { .. } => VerificationState::Done,
        Cancelled(_) => VerificationState::Cancelled,
    })
}

/// The runtime-side half of a verification session. Held by the runtime
/// loop; dropped (cancelling) on logout or when a new session replaces it.
pub struct Session {
    /// Sends UI actions into the driver task.
    actions: UnboundedSender<crate::model::VerificationAction>,
    /// The SDK request, kept so replacement/logout can cancel it
    /// server-side even after the driver task is gone. `None` for a session
    /// that failed to start (nothing to cancel).
    request: Option<VerificationRequest>,
    driver: tokio::task::JoinHandle<()>,
}

impl Session {
    /// Start a session against `target`, publishing progress into `state`.
    /// Awaits only the target resolution (one key query) so a bad target
    /// fails fast into `Failed`; everything after that runs in the spawned
    /// driver.
    pub async fn start(
        client: &Client,
        target: VerificationTarget,
        subject: String,
        state: ClientState,
    ) -> Self {
        let (action_tx, action_rx) = unbounded_channel();
        let mut verification = state.verification;
        verification.set(Some(VerificationSession {
            subject,
            target: target.clone(),
            state: VerificationState::Requested,
            emojis: Vec::new(),
        }));
        // Resolve the target before spawning so a bad target (unknown
        // device / no identity) fails fast into `Failed` instead of
        // leaving a dialog waiting forever. The cancelled-request path
        // then has nothing server-side to cancel, so a dead session is
        // harmless.
        let request = match request_for(client, &target).await {
            Ok(request) => request,
            Err(e) => {
                tracing::warn!("verification failed to start: {e:?}");
                publish(&state, VerificationState::Failed(friendly(&e)), None);
                let driver = tokio::spawn(async {});
                let (dead_tx, _dead_rx) = unbounded_channel();
                return Self {
                    actions: dead_tx,
                    request: None,
                    driver,
                };
            }
        };
        let request_clone = request.clone();
        let driver = tokio::spawn(async move {
            if let Err(e) = drive(request_clone, state, action_rx).await {
                tracing::warn!("verification failed: {e:?}");
                publish(&state, VerificationState::Failed(friendly(&e)), None);
            }
        });
        Self {
            actions: action_tx,
            request: Some(request),
            driver,
        }
    }

    /// Forward a UI decision to the driver task.
    pub fn act(&self, action: crate::model::VerificationAction) {
        let _ = self.actions.send(action);
    }

    /// Cancel and drop the session (logout / replacement). Cancels the SDK
    /// request server-side from a spawned task — the driver task is aborted
    /// immediately and must not be relied on to forward anything.
    pub fn abort(self) {
        if let Some(request) = self.request {
            tokio::spawn(async move {
                if let Err(e) = request.cancel().await {
                    tracing::debug!("verification request cancel failed: {e:?}");
                }
            });
        }
        self.driver.abort();
    }
}

/// Run the state pump for an already-resolved verification request until
/// the session completes.
async fn drive(
    request: VerificationRequest,
    state: ClientState,
    mut actions: UnboundedReceiver<crate::model::VerificationAction>,
) -> Result<(), matrix_sdk::Error> {
    let mut request_changes = request.changes();
    let mut sas_changes: Option<
        std::pin::Pin<
            Box<dyn futures::Stream<Item = matrix_sdk::encryption::verification::SasState> + Send>,
        >,
    > = None;
    let mut current_sas: Option<SasVerification> = None;

    loop {
        let next = tokio::select! {
            change = request_changes.next() => match change {
                Some(change) => RequestOrAction::Request(change),
                None => {
                    // Request stream closed: the request is done or
                    // cancelled; drain any remaining SAS state.
                    RequestOrAction::Done
                }
            },
            action = actions.recv() => match action {
                Some(action) => RequestOrAction::Action(action),
                None => RequestOrAction::Done,
            },
            sas_change = async {
                match sas_changes.as_mut() {
                    Some(stream) => stream.next().await.map(RequestOrAction::Sas).unwrap_or(RequestOrAction::Done),
                    None => std::future::pending().await,
                }
            } => sas_change,
        };
        match next {
            RequestOrAction::Request(change) => {
                use matrix_sdk::encryption::verification::VerificationRequestState::*;
                match change {
                    Created { .. } | Requested { .. } => {
                        // For incoming requests we don't handle: we only
                        // initiate. `Requested` here means the *other* side
                        // responded to our outgoing request... actually the
                        // SDK uses `Requested` for received requests and
                        // `Created` for ours; keep the UI on `Requested`.
                        continue;
                    }
                    Ready { .. } => {
                        // We only ever initiate, so `Ready` means the peer
                        // accepted *our* request — no `accept()` of our own.
                        // Start SAS non-fatally: if the peer already sent
                        // `m.key.verification.start` the request may have
                        // raced ahead to `Transitioned`, which delivers the
                        // SAS through its own arm below.
                        match request.start_sas().await {
                            Ok(Some(sas)) => {
                                sas_changes = Some(Box::pin(sas.changes()));
                                current_sas = Some(sas.clone());
                                if let Some(ui_state) = map_sas_state(&sas) {
                                    publish(&state, ui_state, emojis_from(&sas));
                                }
                            }
                            Ok(None) => {
                                publish(
                                    &state,
                                    VerificationState::Failed(
                                        "The other side doesn't support emoji verification.".into(),
                                    ),
                                    None,
                                );
                                return Ok(());
                            }
                            Err(e) => {
                                tracing::debug!(
                                    "start_sas error (waiting for Transitioned): {e:?}"
                                );
                            }
                        }
                    }
                    Transitioned { verification, .. } => {
                        if let Some(sas) = verification.sas() {
                            // Idempotent: if `Ready` already subscribed us,
                            // this re-grabs the same SAS after a race.
                            sas_changes = Some(Box::pin(sas.changes()));
                            current_sas = Some(sas.clone());
                        }
                    }
                    Done => {
                        publish(&state, VerificationState::Done, None);
                        return Ok(());
                    }
                    Cancelled(_) => {
                        publish(&state, VerificationState::Cancelled, None);
                        return Ok(());
                    }
                }
            }
            RequestOrAction::Sas(sas_state) => {
                use matrix_sdk::encryption::verification::SasState::*;
                match &sas_state {
                    KeysExchanged { .. } => {
                        if let Some(sas) = &current_sas {
                            publish(&state, VerificationState::EmojisShown, emojis_from(sas));
                        }
                    }
                    Confirmed => publish(&state, VerificationState::Confirmed, None),
                    Done { .. } => {
                        publish(&state, VerificationState::Done, None);
                        return Ok(());
                    }
                    Cancelled(info) => {
                        tracing::debug!(?info, "verification cancelled");
                        publish(&state, VerificationState::Cancelled, None);
                        return Ok(());
                    }
                    _ => {}
                }
            }
            RequestOrAction::Action(action) => {
                use crate::model::VerificationAction::*;
                // Pre-SAS window (Requested/Ready): the only meaningful
                // action is cancelling the whole request — dropping it here
                // would leave a live prompt on the peer (review P1).
                let Some(sas) = current_sas.as_ref() else {
                    match action {
                        Confirm => continue,
                        Mismatch | Cancel => {
                            if let Err(e) = request.cancel().await {
                                tracing::debug!("request cancel failed: {e:?}");
                            }
                            publish(&state, VerificationState::Cancelled, None);
                            return Ok(());
                        }
                    }
                };
                let result = match action {
                    Confirm => sas.confirm().await,
                    Mismatch => sas.mismatch().await,
                    Cancel => sas.cancel().await,
                };
                if let Err(e) = result {
                    tracing::warn!("verification action failed: {e:?}");
                    publish(&state, VerificationState::Failed(friendly(&e)), None);
                    return Ok(());
                }
                if matches!(action, Cancel) {
                    publish(&state, VerificationState::Cancelled, None);
                    return Ok(());
                }
            }
            RequestOrAction::Done => {
                return Ok(());
            }
        }
    }
}

enum RequestOrAction {
    Request(matrix_sdk::encryption::verification::VerificationRequestState),
    Sas(matrix_sdk::encryption::verification::SasState),
    Action(crate::model::VerificationAction),
    Done,
}

/// Resolve the verification target to a concrete SDK `VerificationRequest`.
async fn request_for(
    client: &Client,
    target: &VerificationTarget,
) -> Result<VerificationRequest, matrix_sdk::Error> {
    let methods = vec![VerificationMethod::SasV1];
    match target {
        VerificationTarget::Device(device_id) => {
            let own = client
                .user_id()
                .ok_or_else(|| unknown_error("not signed in".into()))?;
            let device_id: OwnedDeviceId = device_id.as_str().into();
            let device: Option<Device> = client.encryption().get_device(own, &device_id).await?;
            match device {
                Some(device) => device
                    .request_verification_with_methods(methods)
                    .await
                    .map_err(|e| unknown_error(e.to_string())),
                None => Err(unknown_error("That session could not be found.".into())),
            }
        }
        VerificationTarget::User(mxid) => {
            let user: OwnedUserId = UserId::parse(mxid.as_str())
                .map_err(|_| unknown_error("invalid user id".into()))?;
            let identity: Option<UserIdentity> =
                client.encryption().get_user_identity(&user).await?;
            match identity {
                Some(identity) => identity
                    .request_verification_with_methods(methods)
                    .await
                    .map_err(|e| unknown_error(e.to_string())),
                None => Err(unknown_error(
                    "No identity known for that user yet — wait for their profile to load.".into(),
                )),
            }
        }
    }
}

/// Fixed, friendly sentences only — SDK errors can quote server responses.
fn friendly(e: &matrix_sdk::Error) -> String {
    let _ = e;
    "Verification failed — try again in a moment.".into()
}
