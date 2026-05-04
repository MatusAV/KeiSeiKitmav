//! Library facade for kei-arch-map. Used by integration tests to exercise
//! evidence checkers without going through the binary. The binary entrypoint
//! lives in `main.rs` and re-exports the same modules privately.

pub mod evidence;
pub mod schema;
