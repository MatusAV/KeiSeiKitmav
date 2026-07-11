//! Platform adapters (per-protocol I/O).
//!
//! `base` defines the [`PlatformAdapter`] trait. `telegram` / `discord` /
//! `slack` are real implementations behind feature gates (see crate
//! `Cargo.toml`); `cli` is always on.

pub mod base;

#[cfg(feature = "cli")]
pub mod cli;

#[cfg(feature = "telegram")]
pub mod telegram;

#[cfg(feature = "discord")]
pub mod discord;

#[cfg(feature = "slack")]
pub mod slack;
