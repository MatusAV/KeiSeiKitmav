//! Export research → markdown / JSON.

use crate::store::ResearchStore;
use anyhow::{anyhow, Result};
use serde_json::json;

pub enum Format {
    Markdown,
    Json,
}

pub fn export(store: &ResearchStore, id: i64, fmt: Format) -> Result<String> {
    let r = store.get_research(id)?.ok_or_else(|| anyhow!("research {id} missing"))?;
    let claims = store.claims_for(id)?;
    let sources = store.sources_for(id)?;
    match fmt {
        Format::Markdown => {
            let mut md = String::new();
            md.push_str(&format!("# Research {}\n\n", r.id));
            md.push_str(&format!("**Query:** {}\n\n", r.query_original));
            md.push_str(&format!("**Status:** {}\n", r.status));
            md.push_str(&format!("**Cost:** {} mc\n\n", r.total_cost_mc));
            md.push_str("## Claims\n\n");
            for c in claims {
                md.push_str(&format!("- [{}] {} (consensus={:.2})\n",
                    c.grade, c.claim_text, c.consensus));
            }
            md.push_str("\n## Sources\n\n");
            if sources.is_empty() {
                md.push_str("_(none)_\n");
            } else {
                for s in &sources {
                    let label = if s.title.is_empty() { &s.domain } else { &s.title };
                    md.push_str(&format!(
                        "- [{:.2}] [{}]({}) — {}\n",
                        s.relevance_score, label, s.url, s.domain,
                    ));
                }
            }
            Ok(md)
        }
        Format::Json => {
            let val = json!({
                "id": r.id,
                "query": r.query_original,
                "status": r.status,
                "cost_mc": r.total_cost_mc,
                "claims": claims,
                "sources": sources,
            });
            Ok(serde_json::to_string_pretty(&val)?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Source;

    fn store_with_one_source() -> (ResearchStore, i64) {
        let store = ResearchStore::open_memory().unwrap();
        let rid = store.create_research("what is the latest rust version").unwrap();
        store.set_status(rid, "completed").unwrap();
        store
            .add_source(&Source {
                research_id: rid,
                url: "https://blog.rust-lang.org/x".into(),
                title: "Rust 1.88".into(),
                content: "release notes".into(),
                provider: "anthropic-websearch".into(),
                domain: "blog.rust-lang.org".into(),
                relevance_score: 0.9,
                ..Default::default()
            })
            .unwrap();
        (store, rid)
    }

    #[test]
    fn markdown_includes_sources() {
        let (store, rid) = store_with_one_source();
        let md = export(&store, rid, Format::Markdown).unwrap();
        assert!(md.contains("## Sources"), "missing Sources heading");
        assert!(md.contains("https://blog.rust-lang.org/x"), "missing url");
        assert!(md.contains("Rust 1.88"), "missing title");
        assert!(md.contains("blog.rust-lang.org"), "missing domain");
    }

    #[test]
    fn json_includes_sources_array() {
        let (store, rid) = store_with_one_source();
        let js = export(&store, rid, Format::Json).unwrap();
        let v: serde_json::Value = serde_json::from_str(&js).unwrap();
        let arr = v["sources"].as_array().expect("sources array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["url"], "https://blog.rust-lang.org/x");
        assert_eq!(arr[0]["provider"], "anthropic-websearch");
    }

    #[test]
    fn markdown_handles_no_sources() {
        let store = ResearchStore::open_memory().unwrap();
        let rid = store.create_research("q").unwrap();
        let md = export(&store, rid, Format::Markdown).unwrap();
        assert!(md.contains("## Sources"));
        assert!(md.contains("_(none)_"));
    }
}
