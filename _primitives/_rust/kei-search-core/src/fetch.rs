//! Source fetcher trait — frozen interface, default impl is a no-op stub.
//!
//! Note: the user-facing `/research` skill does NOT depend on this crate.
//! `/research` runs inside Claude Code and uses the built-in WebFetch /
//! WebSearch tools + parallel Agent spawns; it's fully functional today.
//!
//! `kei-search-core` is a separate scaffold for FUTURE Rust-side automation
//! that needs programmatic web search (e.g. nightly knowledge consolidation
//! without a Claude session). Real providers plug into this trait
//! (anthropic-websearch, SerpAPI, Brave Search API, ...). StubFetcher
//! exists so `cargo build --workspace` stays green while the rest of the
//! crate is being designed.

use crate::types::Source;

/// Implement this trait to integrate a live search provider.
pub trait SourceFetcher {
    /// Fetch sources for `claim`. Returns (source, cost_microcents).
    /// Cost is real — the budget is charged by the pipeline, not by impl.
    fn fetch(&self, claim: &str) -> (Vec<Source>, i64);
}

/// Default stub — returns empty. Frozen interface, no runtime side-effects.
pub struct StubFetcher;

impl SourceFetcher for StubFetcher {
    fn fetch(&self, _claim: &str) -> (Vec<Source>, i64) {
        // Deliberate no-op: this crate ships the frozen `SourceFetcher`
        // interface + research pipeline, but no live web provider is wired
        // yet (see module docs — `/research` uses Claude Code's built-in
        // WebFetch/WebSearch and does NOT depend on this crate). Real
        // providers (anthropic-websearch / SerpAPI / Brave) plug in here.
        (Vec::new(), 0)
    }
}
