//! Runtime bridge between Dioxus (any thread) and matrix-sdk (tokio).
//!
//! matrix-sdk is async and needs a tokio runtime; the UI is not. `ClientRuntime`
//! owns a tokio runtime on a dedicated background thread and accepts commands
//! over an [`UnboundedSender`]. Each command carries a oneshot/callback so the
//! caller can await the result from wherever it lives.
//!
//! Checkpoint 02 starts filling in real commands (`Login`, `SendMessage`, ...);
//! for now only `Ping` exists to prove the round trip works.

use anyhow::Result;
use tokio::{
    runtime::Runtime,
    sync::{mpsc::UnboundedSender, oneshot},
};

/// Commands the UI can send into the Matrix runtime.
pub enum Command {
    /// Connectivity sanity check. Responds with the echoed payload.
    Ping {
        payload: String,
        reply: oneshot::Sender<String>,
    },
}

/// Owns the tokio runtime that matrix-sdk code will run on.
///
/// Dropping this shuts the runtime down; send on the returned sender before
/// dropping if you need a reply.
pub struct ClientRuntime {
    handle: std::thread::JoinHandle<()>,
}

impl ClientRuntime {
    /// Spawn a dedicated thread hosting a multi-threaded tokio runtime that
    /// processes [`Command`]s. Returns the runtime owner and the command sender.
    pub fn spawn() -> (Self, UnboundedSender<Command>) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let handle = std::thread::Builder::new()
            .name("vesper-matrix-runtime".into())
            .spawn(move || {
                let runtime = Runtime::new().expect("failed to build tokio runtime");
                runtime.block_on(async move {
                    while let Some(cmd) = rx.recv().await {
                        match cmd {
                            Command::Ping { payload, reply } => {
                                tracing::debug!(payload = %payload, "ping");
                                let _ = reply.send(format!("pong:{payload}"));
                            }
                        }
                    }
                });
            })
            .expect("failed to spawn matrix runtime thread");
        (ClientRuntime { handle }, tx)
    }

    /// Wait for the runtime thread to finish (e.g. after the sender is dropped).
    pub fn join(self) -> Result<()> {
        self.handle
            .join()
            .map_err(|_| anyhow::anyhow!("matrix runtime thread panicked"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn ping_round_trip() {
        let (runtime, tx) = ClientRuntime::spawn();
        let (reply_tx, reply_rx) = oneshot::channel();
        tx.send(Command::Ping {
            payload: "hello".into(),
            reply: reply_tx,
        })
        .expect("send ping");
        let pong = reply_rx.await.expect("receive pong");
        assert_eq!(pong, "pong:hello");
        drop(tx);
        runtime.join().expect("runtime thread exits cleanly");
    }
}
