//! kei-search-core — 3-wave deep research engine, budget-capped.
//!
//! Waves:
//!  0 — claim extraction from prompt
//!  1 — per-claim source hunt via the [`SourceFetcher`](fetch::SourceFetcher) trait
//!  2 — cross-validation + consensus scoring
//!
//! Port of LBM internal/search. Fetch is a trait the caller supplies:
//! [`AnthropicFetcher`] (live, via the Anthropic web-search tool; opt-in on
//! `ANTHROPIC_API_KEY`) or [`fetch::StubFetcher`] (no-op fallback).

pub mod budget;
pub mod export;
pub mod fetch;
pub mod fetch_anthropic;
pub mod pipeline;
pub mod schema;
pub mod store;
pub mod types;

pub use fetch_anthropic::AnthropicFetcher;
pub use pipeline::run_research;
pub use store::ResearchStore;
pub use types::{Claim, Research, Source};
