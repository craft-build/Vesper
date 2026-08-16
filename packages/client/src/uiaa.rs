//! UIAA password-stage completion (checkpoint 10).
//!
//! Used for deleting sessions: the homeserver answers the first attempt with
//! 401 + a UIAA challenge; we complete the `m.login.password` stage using the
//! password the user typed in the confirm dialog. Credentials never appear in
//! logs or error strings.

use matrix_sdk::{
    ruma::{api::client::uiaa, owned_device_id},
    Client,
};

use crate::api::ClientError;

const GENERIC: &str = "Could not complete the request — try again.";
const BAD_PASSWORD: &str = "Incorrect password — try again.";
const NO_PASSWORD_FLOW: &str =
    "This account does not allow password confirmation (SSO-only homeserver).";

/// Delete `device_id`, completing a `m.login.password` UIAA stage with
/// `password` when the server demands one. A first attempt without auth data
/// both probes the flow and succeeds outright on servers that don't require
/// re-auth (some do for fresh tokens).
pub async fn delete_device_with_password(
    client: &Client,
    device_id: String,
    password: String,
) -> Result<(), ClientError> {
    let devices = vec![owned_device_id!(device_id.as_str())];

    match client.delete_devices(&devices, None).await {
        Ok(_) => Ok(()),
        Err(e) => {
            let Some(info) = e.as_uiaa_response() else {
                return Err(ClientError(GENERIC.into()));
            };
            // SSO-only accounts never offer a password stage: fail with a
            // message the dialog can show instead of a dead retry loop.
            if !password_stage_available(info) {
                return Err(ClientError(NO_PASSWORD_FLOW.into()));
            }
            // Echo the session when the challenge provides one; servers vary.
            let mut stage = uiaa::Password::new(
                uiaa::UserIdentifier::Matrix(uiaa::MatrixUserIdentifier::new(
                    client
                        .user_id()
                        .map(|u| u.to_string())
                        .ok_or_else(|| ClientError("Not signed in.".into()))?,
                )),
                password,
            );
            if let Some(session) = &info.session {
                stage.session = Some(session.clone());
            }
            match client
                .delete_devices(&devices, Some(uiaa::AuthData::Password(stage)))
                .await
            {
                Ok(_) => Ok(()),
                // 401 on the *second* try = wrong password (or a flow we
                // can't complete); anything else is a server/network issue.
                Err(retry) if retry.as_uiaa_response().is_some() => {
                    Err(ClientError(BAD_PASSWORD.into()))
                }
                Err(_) => Err(ClientError(GENERIC.into())),
            }
        }
    }
}

/// Whether the UIAA challenge (if any) offers a `m.login.password` stage —
/// surfaces a friendly error for SSO-only accounts before they type anything.
pub fn password_stage_available(info: &uiaa::UiaaInfo) -> bool {
    info.flows
        .iter()
        .any(|flow| flow.stages.iter().any(|s| *s == "m.login.password".into()))
}
