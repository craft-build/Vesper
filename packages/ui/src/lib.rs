//! Shared Vesper GUI: the Dioxus component tree, mock data layer, and the
//! `VesperClient` trait that lets a future Matrix integration replace the
//! mock data without touching any component.

mod app;
pub use app::App;

pub mod data;

pub mod icons;
mod markdown;
mod window_chrome;

pub mod design_system;

mod chat;
mod screens;
