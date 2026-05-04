//! Predicate → Evidence projection table tests.
//!
//! Covers the §2 mapping table of `arch/MATH-DNA-DESIGN.md`. Each test
//! is one row of that table — flips one predicate variant, asserts the
//! projected evidence kind matches.

use kei_arch_derive::{predicate_to_evidence, EvidenceClaim};
use kei_registry::{Predicate, SymbolKind};
use std::path::PathBuf;

#[test]
fn content_regex_min1_no_max_projects_to_regex_match() {
    let p = Predicate::ContentRegex {
        file: PathBuf::from("README.md"),
        pattern: r"\bRust\b".to_string(),
        min: 1,
        max: None,
    };
    match predicate_to_evidence(&p) {
        EvidenceClaim::RegexMatch { file, pattern } => {
            assert_eq!(file, PathBuf::from("README.md"));
            assert_eq!(pattern, r"\bRust\b");
        }
        other => panic!("expected RegexMatch, got {:?}", other),
    }
}

#[test]
fn content_regex_min_eq_max_projects_to_grep_count() {
    let p = Predicate::ContentRegex {
        file: PathBuf::from("README.md"),
        pattern: "[0-9]+".to_string(),
        min: 3,
        max: Some(3),
    };
    match predicate_to_evidence(&p) {
        EvidenceClaim::GrepCount {
            file,
            pattern,
            expected,
        } => {
            assert_eq!(file, PathBuf::from("README.md"));
            assert_eq!(pattern, "[0-9]+");
            assert_eq!(expected, 3);
        }
        other => panic!("expected GrepCount, got {:?}", other),
    }
}

#[test]
fn file_exists_projects_to_file_exists() {
    let p = Predicate::FileExists {
        path: PathBuf::from("arch/PLAN.toml"),
    };
    match predicate_to_evidence(&p) {
        EvidenceClaim::FileExists { path } => {
            assert_eq!(path, PathBuf::from("arch/PLAN.toml"));
        }
        other => panic!("expected FileExists, got {:?}", other),
    }
}

#[test]
fn cargo_check_projects_to_cargo_check_clean() {
    let p = Predicate::CargoCheck {
        member: "_primitives/_rust".to_string(),
    };
    match predicate_to_evidence(&p) {
        EvidenceClaim::CargoCheckClean { manifest_dir } => {
            assert_eq!(manifest_dir, PathBuf::from("_primitives/_rust"));
        }
        other => panic!("expected CargoCheckClean, got {:?}", other),
    }
}

#[test]
fn http_status_projects_to_http_status() {
    let p = Predicate::HttpStatus {
        url: "https://example.com".to_string(),
        expected: vec![200, 204],
    };
    match predicate_to_evidence(&p) {
        EvidenceClaim::HttpStatus { url, expected } => {
            assert_eq!(url, "https://example.com");
            assert_eq!(expected, vec![200, 204]);
        }
        other => panic!("expected HttpStatus, got {:?}", other),
    }
}

#[test]
fn symbol_declared_fn_synthesizes_pattern() {
    let p = Predicate::SymbolDeclared {
        file: PathBuf::from("src/lib.rs"),
        name: "register_formula".to_string(),
        symbol_kind: SymbolKind::Fn,
    };
    match predicate_to_evidence(&p) {
        EvidenceClaim::RegexMatch { file, pattern } => {
            assert_eq!(file, PathBuf::from("src/lib.rs"));
            assert!(pattern.contains("fn"));
            assert!(pattern.contains("register_formula"));
        }
        other => panic!("expected RegexMatch, got {:?}", other),
    }
}

#[test]
fn body_sha_eq_projects_to_file_exists_sentinel() {
    let p = Predicate::BodyShaEq {
        sha8: "deadbeef".to_string(),
    };
    match predicate_to_evidence(&p) {
        EvidenceClaim::FileExists { path } => {
            assert!(path.display().to_string().contains("deadbeef"));
        }
        other => panic!("expected FileExists, got {:?}", other),
    }
}
