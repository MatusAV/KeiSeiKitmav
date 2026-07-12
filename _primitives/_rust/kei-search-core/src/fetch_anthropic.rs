// SPDX-License-Identifier: Apache-2.0
//! Anthropic web-search-backed [`SourceFetcher`].
//!
//! Calls the Messages API (`POST /v1/messages`) with the server-side
//! `web_search` tool and harvests the returned `web_search_result` items
//! (enriched with citation snippets) into [`Source`]s. Opt-in: constructed
//! only when `ANTHROPIC_API_KEY` is set — otherwise the CLI falls back to the
//! no-op [`crate::fetch::StubFetcher`].
//!
//! No official Anthropic SDK exists for Rust, so this is a raw HTTP call
//! (reqwest blocking — the `SourceFetcher` trait is synchronous). Response
//! parsing is factored into the pure functions [`parse_sources`] /
//! [`estimate_cost_mc`] so it is unit-tested offline against canned JSON.

use crate::fetch::SourceFetcher;
use crate::types::Source;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Default model. The `web_search_20260209` tool requires Opus 4.6+/Sonnet 4.6+;
/// override the model via `KEI_SEARCH_MODEL` (a `web_search_20250305` basic
/// variant would be needed for older models — not wired here).
const DEFAULT_MODEL: &str = "claude-opus-4-8";
const WEB_SEARCH_TOOL_TYPE: &str = "web_search_20260209";
const PROVIDER: &str = "anthropic-websearch";

// Cost model, microcents per token (budget unit: 1 USD = 1_000_000 mc).
// Opus 4.8 list price: $5 / 1M input, $25 / 1M output.
const INPUT_MC_PER_TOKEN: i64 = 5;
const OUTPUT_MC_PER_TOKEN: i64 = 25;
// Web search server-tool: $10 / 1000 searches = 10_000 mc per search.
const SEARCH_MC_PER_USE: i64 = 10_000;

pub struct AnthropicFetcher {
    api_key: String,
    model: String,
    max_uses: u32,
    client: reqwest::blocking::Client,
}

impl AnthropicFetcher {
    /// Build from the environment. Returns `None` when `ANTHROPIC_API_KEY` is
    /// unset/blank (RULE 0.8 — secret via env, never a flag), so callers can
    /// fall back to the stub. `KEI_SEARCH_MODEL` and `KEI_SEARCH_MAX_USES`
    /// tune the request.
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())?;
        let model = std::env::var("KEI_SEARCH_MODEL")
            .ok()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let max_uses = std::env::var("KEI_SEARCH_MAX_USES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .ok()?;
        Some(Self { api_key, model, max_uses, client })
    }

    fn request_body(&self, claim: &str) -> Value {
        json!({
            "model": self.model,
            "max_tokens": 4096,
            "tools": [{
                "type": WEB_SEARCH_TOOL_TYPE,
                "name": "web_search",
                "max_uses": self.max_uses,
            }],
            "messages": [{
                "role": "user",
                "content": format!(
                    "Research this claim and cite primary sources. Be brief. Claim: {claim}"
                ),
            }],
        })
    }
}

impl SourceFetcher for AnthropicFetcher {
    fn fetch(&self, claim: &str) -> (Vec<Source>, i64) {
        let resp = match self
            .client
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&self.request_body(claim))
            .send()
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("kei-search-core: anthropic request failed: {e}");
                return (Vec::new(), 0);
            }
        };
        if !resp.status().is_success() {
            let code = resp.status();
            let body = resp.text().unwrap_or_default();
            let snippet: String = body.chars().take(300).collect();
            eprintln!("kei-search-core: anthropic API {code}: {snippet}");
            return (Vec::new(), 0);
        }
        let v: Value = match resp.json() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("kei-search-core: anthropic response parse failed: {e}");
                return (Vec::new(), 0);
            }
        };
        (parse_sources(&v), estimate_cost_mc(&v))
    }
}

/// Extract the domain (host, sans scheme / path / `www.`) from a URL string,
/// without pulling a URL-parsing crate.
fn domain_of(url: &str) -> String {
    let no_scheme = url.split("://").nth(1).unwrap_or(url);
    let host = no_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(no_scheme);
    host.trim_start_matches("www.").to_string()
}

/// Map every `web_search_result` in a Messages API response to a [`Source`].
///
/// - dedupes by URL,
/// - fills `content` from a matching citation `cited_text` when present, else
///   falls back to the result title,
/// - assigns `relevance_score` by rank (first result = 1.0, descending).
///
/// `id` / `research_id` / `created_at` are left at their defaults — the
/// pipeline sets `research_id` and the store sets `created_at`.
pub fn parse_sources(resp: &Value) -> Vec<Source> {
    let content = match resp.get("content").and_then(Value::as_array) {
        Some(c) => c,
        None => return Vec::new(),
    };

    // url -> first non-empty cited_text (readable snippet for `content`).
    let mut citations: HashMap<String, String> = HashMap::new();
    for block in content {
        let Some(cits) = block.get("citations").and_then(Value::as_array) else { continue };
        for c in cits {
            if let (Some(url), Some(text)) = (
                c.get("url").and_then(Value::as_str),
                c.get("cited_text").and_then(Value::as_str),
            ) {
                if !url.is_empty() && !text.is_empty() {
                    citations.entry(url.to_string()).or_insert_with(|| text.to_string());
                }
            }
        }
    }

    let mut sources: Vec<Source> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for block in content {
        if block.get("type").and_then(Value::as_str) != Some("web_search_tool_result") {
            continue;
        }
        // An error result carries `content` as an object, not an array — skip.
        let Some(results) = block.get("content").and_then(Value::as_array) else { continue };
        for item in results {
            if item.get("type").and_then(Value::as_str) != Some("web_search_result") {
                continue;
            }
            let url = match item.get("url").and_then(Value::as_str) {
                Some(u) if !u.is_empty() => u,
                _ => continue,
            };
            if !seen.insert(url.to_string()) {
                continue;
            }
            let title = item.get("title").and_then(Value::as_str).unwrap_or("").to_string();
            let content_text = citations
                .get(url)
                .cloned()
                .unwrap_or_else(|| title.clone());
            sources.push(Source {
                url: url.to_string(),
                title,
                content: content_text,
                provider: PROVIDER.to_string(),
                domain: domain_of(url),
                relevance_score: 0.0,
                ..Default::default()
            });
        }
    }

    let n = sources.len();
    for (i, s) in sources.iter_mut().enumerate() {
        s.relevance_score = 1.0 - (i as f64) / (n as f64);
    }
    sources
}

/// Estimate the cost of a response in microcents: token usage priced at the
/// Opus-tier list rate plus a per-search charge. Reads
/// `usage.server_tool_use.web_search_requests` when present, else counts
/// `web_search_tool_result` blocks.
pub fn estimate_cost_mc(resp: &Value) -> i64 {
    let usage = resp.get("usage");
    let input = usage
        .and_then(|u| u.get("input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output = usage
        .and_then(|u| u.get("output_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let searches = usage
        .and_then(|u| u.get("server_tool_use"))
        .and_then(|s| s.get("web_search_requests"))
        .and_then(Value::as_i64)
        .unwrap_or_else(|| count_search_result_blocks(resp));
    input * INPUT_MC_PER_TOKEN + output * OUTPUT_MC_PER_TOKEN + searches * SEARCH_MC_PER_USE
}

fn count_search_result_blocks(resp: &Value) -> i64 {
    resp.get("content")
        .and_then(Value::as_array)
        .map(|c| {
            c.iter()
                .filter(|b| {
                    b.get("type").and_then(Value::as_str) == Some("web_search_tool_result")
                })
                .count() as i64
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canned_response() -> Value {
        json!({
            "content": [
                {
                    "type": "text",
                    "text": "Per the source, the sky is blue.",
                    "citations": [
                        {
                            "type": "web_search_result_location",
                            "url": "https://www.example.com/sky?utm=1",
                            "title": "Why the sky is blue",
                            "cited_text": "Rayleigh scattering makes the sky blue."
                        }
                    ]
                },
                {
                    "type": "web_search_tool_result",
                    "content": [
                        {
                            "type": "web_search_result",
                            "url": "https://www.example.com/sky?utm=1",
                            "title": "Why the sky is blue"
                        },
                        {
                            "type": "web_search_result",
                            "url": "https://noaa.gov/atmosphere",
                            "title": "Atmosphere basics"
                        },
                        {
                            "type": "web_search_result",
                            "url": "https://www.example.com/sky?utm=1",
                            "title": "duplicate — should be dropped"
                        }
                    ]
                }
            ],
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 200,
                "server_tool_use": { "web_search_requests": 2 }
            }
        })
    }

    #[test]
    fn parses_dedupes_and_ranks_sources() {
        let sources = parse_sources(&canned_response());
        assert_eq!(sources.len(), 2, "third result is a dup URL and must drop");

        let first = &sources[0];
        assert_eq!(first.url, "https://www.example.com/sky?utm=1");
        assert_eq!(first.title, "Why the sky is blue");
        assert_eq!(first.provider, "anthropic-websearch");
        assert_eq!(first.domain, "example.com", "strips scheme, www., query");
        // content comes from the matching citation, not the title
        assert_eq!(first.content, "Rayleigh scattering makes the sky blue.");
        assert!((first.relevance_score - 1.0).abs() < 1e-9, "first ranks highest");

        let second = &sources[1];
        assert_eq!(second.domain, "noaa.gov");
        // no citation for this url → content falls back to the title
        assert_eq!(second.content, "Atmosphere basics");
        assert!((second.relevance_score - 0.5).abs() < 1e-9);
    }

    #[test]
    fn cost_is_tokens_plus_searches() {
        // 1000*5 + 200*25 + 2*10_000 = 5000 + 5000 + 20_000 = 30_000 mc
        assert_eq!(estimate_cost_mc(&canned_response()), 30_000);
    }

    #[test]
    fn cost_counts_blocks_when_usage_lacks_search_count() {
        let v = json!({
            "content": [{ "type": "web_search_tool_result", "content": [] }],
            "usage": { "input_tokens": 0, "output_tokens": 0 }
        });
        // no server_tool_use → falls back to counting the one result block
        assert_eq!(estimate_cost_mc(&v), SEARCH_MC_PER_USE);
    }

    #[test]
    fn empty_or_error_shapes_yield_no_sources() {
        assert!(parse_sources(&json!({})).is_empty());
        // error result: content is an object, not an array
        let err = json!({"content": [{
            "type": "web_search_tool_result",
            "content": { "type": "web_search_tool_result_error", "error_code": "max_uses_exceeded" }
        }]});
        assert!(parse_sources(&err).is_empty());
    }

    #[test]
    fn domain_extraction() {
        assert_eq!(domain_of("https://www.foo.com/a/b?x=1"), "foo.com");
        assert_eq!(domain_of("http://bar.org"), "bar.org");
        assert_eq!(domain_of("no-scheme.example/x"), "no-scheme.example");
    }
}
