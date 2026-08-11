//! Matrix bridge for Vesper.
//!
//! This crate owns everything that talks to a Matrix homeserver via
//! `matrix-sdk`. It is deliberately not wired into `ui` yet (checkpoint 02);
//! it only needs to compile and pass its own unit tests.

pub mod runtime;
