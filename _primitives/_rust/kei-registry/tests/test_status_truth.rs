//! Phase 3 Layer 3 — STATUS-TRUTH MARKER parser + registry-insert tests.
//!
//! Covers RULE 0.16 marker happy paths, malformed-rejection, severity
//! mapping, and the register/SELECT roundtrip via cleanup_findings.

use kei_registry::status_truth::{
    ensure_schema, parse_marker, register, severity_of, BoolOrNa, CheckResult, ShippedKind,
};
use rusqlite::Connection;
use tempfile::tempdir;

const FUNCTIONAL_OK: &str = "noise before
=== STATUS-TRUTH MARKER ===
shipped: functional
stubs: 0
cargo-check: PASS
behaviour-verified: yes
follow-up-required:
";

const SCAFFOLDING_WITH_STUBS: &str = "=== STATUS-TRUTH MARKER ===
shipped: scaffolding
stubs: 3 src/foo.rs:12, src/bar.rs:44, src/baz.rs:7
cargo-check: NOT-RUN
behaviour-verified: no
follow-up-required:
  - implement foo body
  - wire bar caller
  - test baz path
";

const PARTIAL_BUNDLE: &str = "=== STATUS-TRUTH MARKER ===
shipped: partial
stubs: 1
cargo-check: PASS
behaviour-verified: not-applicable
follow-up-required:
  - finish second handler
";

const MALFORMED_NO_SHIPPED: &str = "=== STATUS-TRUTH MARKER ===
stubs: 0
cargo-check: PASS
behaviour-verified: yes
";

#[test]
fn parse_marker_functional() {
    let m = parse_marker(FUNCTIONAL_OK).unwrap();
    assert_eq!(m.shipped, ShippedKind::Functional);
    assert_eq!(m.stubs_count, 0);
    assert!(m.stubs_locations.is_empty());
    assert_eq!(m.cargo_check, CheckResult::Pass);
    assert_eq!(m.behaviour_verified, BoolOrNa::Yes);
    assert!(m.follow_up_required.is_empty());
}

#[test]
fn parse_marker_scaffolding_with_stubs() {
    let m = parse_marker(SCAFFOLDING_WITH_STUBS).unwrap();
    assert_eq!(m.shipped, ShippedKind::Scaffolding);
    assert_eq!(m.stubs_count, 3);
    assert_eq!(m.stubs_locations.len(), 3);
    assert!(m.stubs_locations[0].contains("foo.rs:12"));
    assert_eq!(m.cargo_check, CheckResult::NotRun);
    assert_eq!(m.behaviour_verified, BoolOrNa::No);
    assert_eq!(m.follow_up_required.len(), 3);
    assert_eq!(m.follow_up_required[1], "wire bar caller");
}

#[test]
fn parse_marker_rejects_malformed() {
    let err = parse_marker(MALFORMED_NO_SHIPPED).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("shipped"), "error mentions missing field: {msg}");
}

#[test]
fn severity_mapping_matches_spec() {
    let scaffold = parse_marker(SCAFFOLDING_WITH_STUBS).unwrap();
    let partial = parse_marker(PARTIAL_BUNDLE).unwrap();
    let functional = parse_marker(FUNCTIONAL_OK).unwrap();
    assert_eq!(severity_of(&scaffold), "high");
    assert_eq!(severity_of(&partial), "medium");
    assert_eq!(severity_of(&functional), "info");
}

#[test]
fn register_inserts_with_severity_high_for_scaffolding() {
    let tmp = tempdir().unwrap();
    let db = tmp.path().join("reg.sqlite");
    let conn = Connection::open(&db).unwrap();
    let m = parse_marker(SCAFFOLDING_WITH_STUBS).unwrap();
    let inserted = register(&conn, "agent-foo-12345", &m).unwrap();
    assert!(inserted);
    let row: (String, String, String) = conn
        .query_row(
            "SELECT workspace_sha, severity, kind FROM cleanup_findings",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(row.0, "agent-foo-12345");
    assert_eq!(row.1, "high");
    assert_eq!(row.2, "agent_status_truth");
}

#[test]
fn register_skips_functional_zero_stubs() {
    let tmp = tempdir().unwrap();
    let db = tmp.path().join("reg.sqlite");
    let conn = Connection::open(&db).unwrap();
    ensure_schema(&conn).unwrap();
    let m = parse_marker(FUNCTIONAL_OK).unwrap();
    let inserted = register(&conn, "agent-bar-67890", &m).unwrap();
    assert!(!inserted, "functional + zero stubs must skip insert");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM cleanup_findings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn roundtrip_via_register_and_select() {
    let tmp = tempdir().unwrap();
    let db = tmp.path().join("reg.sqlite");
    let conn = Connection::open(&db).unwrap();
    let m1 = parse_marker(SCAFFOLDING_WITH_STUBS).unwrap();
    let m2 = parse_marker(PARTIAL_BUNDLE).unwrap();
    register(&conn, "agent-1", &m1).unwrap();
    register(&conn, "agent-2", &m2).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM cleanup_findings WHERE kind='agent_status_truth'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    let high: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM cleanup_findings WHERE severity='high'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(high, 1);
    let medium: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM cleanup_findings WHERE severity='medium'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(medium, 1);
}
