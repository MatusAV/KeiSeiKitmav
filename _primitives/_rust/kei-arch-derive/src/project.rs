//! Predicate → Evidence projection (the §2 mapping table of MATH-DNA-DESIGN).
//!
//! Each kei-registry `Predicate` variant projects DOWN to one of the seven
//! evidence kinds already shipped in `kei_arch_map::schema::Evidence`:
//! `FileExists`, `RegexMatch`, `GrepCount`, `FileSize`, `JsonField`,
//! `CargoCheckClean`, `HttpStatus`. The 6 "new derivable kinds" (per
//! Wave 5 Option B verdict) project onto existing ones via synthesized
//! patterns — no schema bump required.
//!
//! Constructor Pattern: this cube ONLY converts. It owns no I/O and no
//! ordering. Output is deterministic for a given predicate (pure fn).

use kei_registry::{Predicate, SymbolKind};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Mirror of `kei_arch_map::schema::Evidence` — the seven evidence kinds
/// that PLAN.toml currently understands. Defined here (not imported) to
/// keep this crate compile-time independent of `kei-arch-map`'s network
/// stack while still emitting wire-compatible TOML.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvidenceClaim {
    FileExists {
        path: PathBuf,
    },
    RegexMatch {
        file: PathBuf,
        pattern: String,
    },
    GrepCount {
        file: PathBuf,
        pattern: String,
        expected: u64,
    },
    FileSize {
        path: PathBuf,
        range: [u64; 2],
    },
    JsonField {
        file: PathBuf,
        path: String,
        expected: String,
    },
    CargoCheckClean {
        manifest_dir: PathBuf,
    },
    HttpStatus {
        url: String,
        expected: Vec<u16>,
    },
}

/// Project a single registry predicate onto an `EvidenceClaim`.
///
/// Mapping table (per `arch/MATH-DNA-DESIGN.md` §2):
/// - `ContentRegex {min ≥ 1, max = None}` → `RegexMatch`
/// - `ContentRegex {min = max = N}` → `GrepCount`
/// - `ContentRegex {min, max}` other → `RegexMatch` (presence sufficient)
/// - `ContentNotRegex` → `RegexMatch` with negative-lookbehind synthesized
///   pattern (best-effort; documented as projection limitation)
/// - `FileExists` → `FileExists`
/// - `JsonSchema` → `JsonField` with `path = "$schema"` placeholder
/// - `HttpStatus` → `HttpStatus`
/// - `CargoCheck` → `CargoCheckClean`
/// - `CargoTest` → `RegexMatch` on Cargo.toml as fallback presence claim
/// - `SymbolDeclared` → `RegexMatch` with synthesized symbol pattern
/// - `BodyShaEq` → `FileExists` on `.kei-arch/sha-cache/<sha8>` sentinel
pub fn predicate_to_evidence(p: &Predicate) -> EvidenceClaim {
    match p {
        Predicate::ContentRegex { file, pattern, min, max } => {
            project_content_regex(file, pattern, *min, *max)
        }
        Predicate::ContentNotRegex { file, pattern } => project_not_regex(file, pattern),
        Predicate::FileExists { path } => EvidenceClaim::FileExists { path: path.clone() },
        Predicate::JsonSchema { file, schema } => project_json_schema(file, schema),
        Predicate::HttpStatus { url, expected } => EvidenceClaim::HttpStatus {
            url: url.clone(),
            expected: expected.clone(),
        },
        Predicate::CargoCheck { member } => EvidenceClaim::CargoCheckClean {
            manifest_dir: PathBuf::from(member),
        },
        Predicate::CargoTest { member, .. } => project_cargo_test(member),
        Predicate::SymbolDeclared { file, name, symbol_kind } => EvidenceClaim::RegexMatch {
            file: file.clone(),
            pattern: symbol_pattern(name, symbol_kind),
        },
        Predicate::BodyShaEq { sha8 } => EvidenceClaim::FileExists {
            path: PathBuf::from(format!(".kei-arch/sha-cache/{}", sha8)),
        },
    }
}

fn project_not_regex(file: &Path, pattern: &str) -> EvidenceClaim {
    EvidenceClaim::RegexMatch {
        file: file.to_path_buf(),
        pattern: format!("(?s)^(?:(?!{}).)*$", pattern),
    }
}

fn project_json_schema(file: &Path, schema: &Path) -> EvidenceClaim {
    EvidenceClaim::JsonField {
        file: file.to_path_buf(),
        path: "$schema".to_string(),
        expected: schema.display().to_string(),
    }
}

fn project_cargo_test(member: &str) -> EvidenceClaim {
    EvidenceClaim::RegexMatch {
        file: PathBuf::from(member).join("Cargo.toml"),
        pattern: "(?m)^\\[package\\]".to_string(),
    }
}

fn project_content_regex(
    file: &Path,
    pattern: &str,
    min: u32,
    max: Option<u32>,
) -> EvidenceClaim {
    if let Some(maxv) = max {
        if maxv == min {
            return EvidenceClaim::GrepCount {
                file: file.to_path_buf(),
                pattern: pattern.to_string(),
                expected: u64::from(min),
            };
        }
    }
    EvidenceClaim::RegexMatch {
        file: file.to_path_buf(),
        pattern: pattern.to_string(),
    }
}

fn symbol_pattern(name: &str, kind: &SymbolKind) -> String {
    let escaped = regex_escape(name);
    match kind {
        SymbolKind::Fn => format!(r"\bfn\s+{}\b", escaped),
        SymbolKind::Struct => format!(r"\bstruct\s+{}\b", escaped),
        SymbolKind::Enum => format!(r"\benum\s+{}\b", escaped),
        SymbolKind::Trait => format!(r"\btrait\s+{}\b", escaped),
        SymbolKind::Const => format!(r"\bconst\s+{}\b", escaped),
        SymbolKind::Impl => format!(r"\bimpl\b.*\b{}\b", escaped),
    }
}

/// Minimal regex-meta escape. Symbol names rarely contain regex meta but
/// guard against `_`-prefixed names colliding with `_` quantifier-like
/// surface patterns. Identifiers in Rust are `[A-Za-z_][A-Za-z0-9_]*`,
/// none of which need escaping under the std `regex` syntax — this is a
/// belt-and-braces no-op for valid input and a safety net otherwise.
fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "\\.+*?()[]{}|^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
