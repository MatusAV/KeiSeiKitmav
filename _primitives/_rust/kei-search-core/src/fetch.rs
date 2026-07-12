//! Source fetcher trait — frozen interface, default impl is a no-op stub.
//!
//! Note: the user-facing `/research` skill does NOT depend on this crate.
//! `/research` runs inside Claude Code and uses the built-in WebFetch /
//! WebSearch tools + parallel Agent spawns; it's fully functional today.
//!
//! `kei-search-core` is a separate scaffold for Rust-side automation that
//! needs programmatic web search (e.g. nightly knowledge consolidation
//! without a Claude session). The live provider is
//! [`AnthropicFetcher`](crate::fetch_anthropic::AnthropicFetcher) (Anthropic
//! web-search tool; opt-in on `ANTHROPIC_API_KEY`); `StubFetcher` is the
//! no-op fallback when no key is configured.

use crate::types::Source;

/// Implement this trait to integrate a live search provider.
pub trait SourceFetcher {
    /// Fetch sources for `claim`. Returns (source, cost_microcents).
    /// Cost is real — the budget is charged by the pipeline, not by impl.
    fn fetch(&self, claim: &str) -> (Vec<Source>, i64);
}

/// No-op fallback — returns empty, no runtime side-effects. Used when
/// `ANTHROPIC_API_KEY` is unset so the pipeline still runs offline. The live
/// path is [`AnthropicFetcher`](crate::fetch_anthropic::AnthropicFetcher).
pub struct StubFetcher;

impl SourceFetcher for StubFetcher {
    fn fetch(&self, _claim: &str) -> (Vec<Source>, i64) {
        (Vec::new(), 0)
    }
}
