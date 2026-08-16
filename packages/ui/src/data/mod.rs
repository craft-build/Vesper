//! Everything screens need from "a backend": the `VesperClient` seam, its
//! domain types, and the mock implementation.
//!
//! The trait + domain types live in the `client` crate (re-exported here) so
//! the real `MatrixClient` can implement them without a circular crate
//! dependency — the SDK itself never links here; components only see this
//! module.

use std::rc::Rc;

mod mock;

pub use client::{
    api::{ClientError, ClientErrorKind, ClientState, VesperClient},
    model::*,
};
pub use mock::MockClient;

/// The single place where the backend is chosen (called once by
/// [`crate::app::App`]). On native targets `VESPER_BACKEND=mock` picks the
/// mock; anything else (default included) builds the real Matrix client. On
/// wasm the mock is always used — matrix-sdk's sqlite store is not wired up
/// for the web target yet.
#[cfg(not(target_arch = "wasm32"))]
pub fn backend() -> Rc<dyn VesperClient> {
    match std::env::var("VESPER_BACKEND").as_deref() {
        Ok("mock") | Ok("MOCK") => Rc::new(MockClient::default()),
        _ => Rc::new(client::MatrixClient::new()),
    }
}

#[cfg(target_arch = "wasm32")]
pub fn backend() -> Rc<dyn VesperClient> {
    Rc::new(MockClient::default())
}
