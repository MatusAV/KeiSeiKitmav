//! Integration tests for keidocs extractors and DNA stability.

use keidocs::dna::compute_dna;
use keidocs::extractor::{extract_jsdoc, extract_rustdoc, SectionKind};

#[test]
fn rustdoc_picks_up_module_and_item_docs() {
    let src = r#"//! Module-level docs line one.
//! Module-level docs line two.

/// Adds two integers.
pub fn add(a: i32, b: i32) -> i32 { a + b }

fn private_no_doc() {}
"#;
    let sections = extract_rustdoc(src);
    assert!(sections.iter().any(|s| s.kind == SectionKind::Module));
    let items: Vec<_> = sections.iter().filter(|s| s.kind == SectionKind::Item).collect();
    assert_eq!(items.len(), 1);
    assert!(items[0].body.contains("Adds two integers"));
    assert!(items[0]
        .target
        .as_deref()
        .unwrap_or("")
        .contains("add"));
}

#[test]
fn jsdoc_extracts_block_comments() {
    let src = r#"/**
 * Returns the answer.
 * @returns number
 */
function answer() { return 42; }
"#;
    let out = extract_jsdoc(src);
    assert_eq!(out.len(), 1);
    assert!(out[0].body.contains("Returns the answer"));
}

#[test]
fn dna_is_deterministic() {
    let a = compute_dna("a/b.rs", "code", &["serde".into(), "anyhow".into()]);
    let b = compute_dna("a/b.rs", "code", &["anyhow".into(), "serde".into()]);
    assert_eq!(a, b, "dna must be order-independent over deps");
    let c = compute_dna("a/b.rs", "code-changed", &["serde".into(), "anyhow".into()]);
    assert_ne!(a, c, "dna must change with content");
}
