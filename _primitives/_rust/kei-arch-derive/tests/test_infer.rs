//! Phase 2 PR-4 — body→formula inference tests.
//!
//! Each test exercises one row of the §1.2 regex table or one round-trip
//! step of the persistence path. Constructor Pattern: each helper is one
//! cube; tests assert one observation each.

use kei_arch_derive::infer::{
    body_sha8, build_formula, confidence_score, infer_effects, run,
};
use kei_registry::{open_db, EffectKind, FormulaSource};
use std::collections::BTreeSet;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn infer_detects_fs_write() {
    let body = r#"fn save(p: &Path) { std::fs::write(p, b"x").unwrap(); }"#;
    let effects = infer_effects(body);
    let has_fs_write = effects
        .iter()
        .any(|e| matches!(e, EffectKind::FsWrite { .. }));
    assert!(has_fs_write, "expected FsWrite, got {:?}", effects);
}

#[test]
fn infer_detects_exec() {
    let body = r#"let _ = std::process::Command::new("ls").output();"#;
    let effects = infer_effects(body);
    let has_exec = effects.iter().any(|e| matches!(e, EffectKind::Exec { .. }));
    assert!(has_exec, "expected Exec, got {:?}", effects);
}

#[test]
fn infer_detects_multiple_effects_in_one_body() {
    let body = r#"
        std::fs::read("/etc/hostname")?;
        std::fs::write("/tmp/x", b"y")?;
        std::env::var("HOME").unwrap();
    "#;
    let effects = infer_effects(body);
    let has_read = effects.iter().any(|e| matches!(e, EffectKind::FsRead { .. }));
    let has_write = effects
        .iter()
        .any(|e| matches!(e, EffectKind::FsWrite { .. }));
    let has_env = effects
        .iter()
        .any(|e| matches!(e, EffectKind::EnvRead { .. }));
    assert!(has_read && has_write && has_env, "got {:?}", effects);
}

#[test]
fn infer_skips_empty_body() {
    let effects = infer_effects("");
    assert_eq!(effects, BTreeSet::new());
}

#[test]
fn infer_does_not_match_unrelated_text() {
    // Plain prose: should not trigger any pattern.
    let body = "This is documentation describing how a hypothetical system would work.";
    let effects = infer_effects(body);
    assert_eq!(effects, BTreeSet::new());
}

#[test]
fn body_sha8_is_eight_hex_chars() {
    let sha = body_sha8("hello world");
    assert_eq!(sha.len(), 8);
    assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn body_sha8_changes_with_content() {
    assert_ne!(body_sha8("a"), body_sha8("b"));
    assert_eq!(body_sha8("a"), body_sha8("a"));
}

#[test]
fn confidence_score_monotonic_on_length() {
    let small = "x".repeat(50);
    let medium = "x".repeat(500);
    let large = "x".repeat(5000);
    let s = confidence_score(&small);
    let m = confidence_score(&medium);
    let l = confidence_score(&large);
    assert!(s <= m && m <= l, "s={s} m={m} l={l} not monotonic");
    assert!(l <= 100);
}

#[test]
fn build_formula_marks_source_inferred() {
    let formula = build_formula(42, Path::new("/tmp/x.rs"), "std::fs::write(...)");
    match formula.source {
        FormulaSource::Inferred { confidence: _ } => {}
        other => panic!("expected Inferred, got {:?}", other),
    }
    assert_eq!(formula.block_id, 42);
    assert!(!formula.invariants.is_empty(), "invariants must be set");
}

#[test]
fn run_inference_round_trips_via_registry() {
    let tmp = TempDir::new().unwrap();
    // Stage a fake workspace with one Rust crate body.
    let crate_src = tmp
        .path()
        .join("_primitives")
        .join("_rust")
        .join("fakecrate")
        .join("src");
    std::fs::create_dir_all(&crate_src).unwrap();
    std::fs::write(
        crate_src.join("lib.rs"),
        b"pub fn save() { std::fs::write(\"/tmp/x\", b\"y\").unwrap(); }",
    )
    .unwrap();
    // Run the inference pass against a fresh registry DB.
    let db = tmp.path().join("registry.sqlite");
    let count = run(tmp.path(), &db).unwrap();
    assert!(count >= 1, "expected at least one formula registered");
    // Verify the row is present and Inferred.
    let conn = open_db(&db).unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM blocks WHERE formula_source LIKE '%inferred%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(n >= 1, "expected ≥1 inferred formula row, got {n}");
}

#[test]
fn run_inference_skips_empty_workspace() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("registry.sqlite");
    let count = run(tmp.path(), &db).unwrap();
    assert_eq!(count, 0);
}
