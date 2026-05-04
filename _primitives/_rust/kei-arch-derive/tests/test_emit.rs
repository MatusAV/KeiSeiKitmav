//! Emit determinism tests — same input produces byte-identical output,
//! and the inline-table shape is parseable as TOML.

use kei_arch_derive::emit::derive_plan;
use kei_arch_derive::{render_plan_string, FormulaDecl};
use kei_registry::Predicate;
use std::path::PathBuf;

fn fixture_decl() -> FormulaDecl {
    FormulaDecl {
        package_name: "kei-example".to_string(),
        manifest_dir: PathBuf::from("_primitives/_rust/kei-example"),
        effects: vec!["FsRead:src/**".to_string()],
        invariants: vec![Predicate::FileExists {
            path: PathBuf::from("Cargo.toml"),
        }],
    }
}

#[test]
fn empty_decls_emit_meta_only_block() {
    let plan = derive_plan(&[], "https://example.com/blob/main/");
    let s = render_plan_string(&plan);
    assert!(s.contains("[meta]"));
    assert!(s.contains("schema_version = 1"));
    assert!(!s.contains("[[module]]"));
    let parsed: toml::Value = toml::from_str(&s).expect("rendered output must be valid TOML");
    assert!(parsed.get("meta").is_some());
}

#[test]
fn render_is_byte_deterministic() {
    let decls = vec![fixture_decl()];
    let plan_a = derive_plan(&decls, "https://example.com/blob/main/");
    let plan_b = derive_plan(&decls, "https://example.com/blob/main/");
    let a = render_plan_string(&plan_a);
    let b = render_plan_string(&plan_b);
    assert_eq!(a, b, "render must be deterministic across calls");
}

#[test]
fn rendered_plan_is_valid_toml_with_module_block() {
    let decls = vec![fixture_decl()];
    let plan = derive_plan(&decls, "https://example.com/blob/main/");
    let s = render_plan_string(&plan);
    let parsed: toml::Value = toml::from_str(&s).expect("must be valid TOML");
    let modules = parsed
        .get("module")
        .and_then(|m| m.as_array())
        .expect("modules array");
    assert_eq!(modules.len(), 1);
    let m = &modules[0];
    assert_eq!(m.get("id").and_then(|v| v.as_str()), Some("kei-example"));
    let claims = m
        .get("claim")
        .and_then(|c| c.as_array())
        .expect("claims array");
    assert_eq!(claims.len(), 1);
    let evidence = claims[0]
        .get("evidence")
        .and_then(|e| e.as_table())
        .expect("evidence table");
    assert_eq!(
        evidence.get("kind").and_then(|v| v.as_str()),
        Some("file_exists")
    );
}

#[test]
fn modules_are_sorted_by_id() {
    let decl_b = FormulaDecl {
        package_name: "z-late".to_string(),
        manifest_dir: PathBuf::from("z"),
        effects: vec![],
        invariants: vec![],
    };
    let decl_a = FormulaDecl {
        package_name: "a-early".to_string(),
        manifest_dir: PathBuf::from("a"),
        effects: vec![],
        invariants: vec![],
    };
    let plan = derive_plan(&[decl_b, decl_a], "https://x");
    assert_eq!(plan.modules[0].id, "a-early");
    assert_eq!(plan.modules[1].id, "z-late");
}

#[test]
fn header_comment_is_present_for_auditability() {
    let plan = derive_plan(&[], "https://x");
    let s = render_plan_string(&plan);
    assert!(s.starts_with("# AUTO-GENERATED"));
    assert!(s.contains("Do NOT hand-edit"));
}
