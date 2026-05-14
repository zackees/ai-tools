//! `meta-hook` library crate.
//!
//! The binary in `main.rs` is a thin orchestration layer; all of the
//! delegation logic lives in these modules so it can be exercised by both
//! `cargo test --lib` and the integration tests in `tests/`.

pub mod cli;
pub mod discover;
pub mod dispatch;
pub mod envelope;
pub mod settings;
pub mod trust;
