//! Phase 2 PR-4 — inference pass: walk repo, regex-match block bodies,
//! emit `Inferred` BlockFormula. Per `arch/MATH-DNA-DESIGN.md` §1.2 regex
//! table.
//!
//! Constructor Pattern: this cube ONLY does the body→formula projection.
//! Walking lives in `walker::walk_blocks`; persistence in `kei_registry`.
//! Each helper is one cube with one responsibility, all <30 LOC.

use crate::walker::walk_blocks;
use anyhow::Result;
use kei_registry::{
    open_db, register, register_formula, BlockFormula, BlockType, EffectKind, FormulaSource,
    Predicate, TypeAtom, TypeSignature,
};
use std::collections::BTreeSet;
use std::path::Path;

/// Walk `workspace`, infer one `BlockFormula` per non-empty block body, and
/// register it into the kei-registry SQLite at `registry_db`. Returns the
/// number of formulas registered (skips bodies with no inferred effects
/// AND no invariants — the empty-formula filter).
pub fn run(workspace: &Path, registry_db: &Path) -> Result<usize> {
    let conn = open_db(registry_db)?;
    let mut count = 0;
    for entry in walk_blocks(workspace)? {
        let body = std::fs::read_to_string(&entry).unwrap_or_default();
        if body.is_empty() {
            continue;
        }
        let block_type = block_type_for(&entry);
        let path_str = entry.to_string_lossy().to_string();
        let name = entry
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed")
            .to_string();
        let block = register(&conn, block_type, &name, &path_str, body.as_bytes(), "")?;
        let formula = build_formula(block.id, &entry, &body);
        if formula.effects.is_empty() && formula.invariants.is_empty() {
            continue;
        }
        register_formula(&conn, &formula)?;
        count += 1;
    }
    Ok(count)
}

/// Build an `Inferred` formula from one block path + body. Type signature
/// is `Unit -> Unit` (no static signature inference yet); effects come from
/// the §1.2 regex table; invariants record the body-sha lock + a self
/// `FileExists` claim.
pub fn build_formula(block_id: i64, block_path: &Path, body: &str) -> BlockFormula {
    BlockFormula {
        block_id,
        r#type: TypeSignature {
            inputs: vec![],
            output: TypeAtom::Unit,
            errors: vec![],
        },
        invariants: infer_invariants(block_path, body),
        effects: infer_effects(body),
        deps: BTreeSet::new(),
        source: FormulaSource::Inferred {
            confidence: confidence_score(body),
        },
    }
}

/// Apply the §1.2 regex table to a block body. Returns the unique set of
/// inferred `EffectKind`s. Each pattern group maps onto exactly one effect
/// variant; multiple matches in one body collapse into one set entry.
pub fn infer_effects(body: &str) -> BTreeSet<EffectKind> {
    let mut out = BTreeSet::new();
    if matches_any(body, &[r"std::fs::write", r"tokio::fs::write", r"fs\.writeFileSync"]) {
        out.insert(EffectKind::FsWrite { glob: "*".into() });
    }
    if matches_any(body, &[r"std::fs::read", r"tokio::fs::read", r"fs\.readFile"]) {
        out.insert(EffectKind::FsRead { glob: "*".into() });
    }
    if matches_any(body, &[r"std::env::var", r"os\.environ", r"\bgetenv\b"]) {
        out.insert(EffectKind::EnvRead { var: "*".into() });
    }
    if matches_any(body, &[r"reqwest::", r"hyper::client", r"\bfetch\("]) {
        out.insert(EffectKind::NetEgress { host_glob: "*".into() });
    }
    if matches_any(body, &[r"std::process::Command"]) {
        out.insert(EffectKind::Exec { binary: "*".into() });
    }
    if matches_any(body, &[r"git\s+(add|commit|push|reset)"]) {
        out.insert(EffectKind::GitMutate);
    }
    if matches_any(body, &[r"rusqlite::Connection::open", r"sqlx::"]) {
        out.insert(EffectKind::DbWrite { backend: "sqlite".into() });
    }
    out
}

/// Returns true iff any of `patterns` (as regex) finds at least one match
/// in `body`. Compilation failures degrade to "no match" — preserves the
/// inference pass even if a pattern is malformed.
fn matches_any(body: &str, patterns: &[&str]) -> bool {
    patterns
        .iter()
        .any(|p| regex::Regex::new(p).map(|r| r.is_match(body)).unwrap_or(false))
}

/// Self-claim invariants — every inferred block carries a `BodyShaEq` lock
/// (tamper-detect for the inferred formula) and a `FileExists` predicate
/// pointing at its own source.
pub fn infer_invariants(block_path: &Path, body: &str) -> Vec<Predicate> {
    vec![
        Predicate::BodyShaEq {
            sha8: body_sha8(body),
        },
        Predicate::FileExists {
            path: block_path.to_path_buf(),
        },
    ]
}

/// 8-hex-char prefix of SHA-256(body). Matches the registry's body-sha
/// length convention (see `kei_registry::dna_block::short_sha8`).
pub fn body_sha8(body: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(body.as_bytes());
    let d = h.finalize();
    format!("{:02x}{:02x}{:02x}{:02x}", d[0], d[1], d[2], d[3])
}

/// Confidence is a coarse function of body length — longer bodies have
/// more pattern surface, so an inferred subset is more likely to be the
/// full effect set. Bands chosen to keep the score in 0..100.
pub fn confidence_score(body: &str) -> u8 {
    let len = body.len();
    if len < 100 {
        30
    } else if len < 1000 {
        60
    } else {
        80
    }
}

/// Map a file path to one of the five registry block types. Heuristic by
/// extension — `.rs` → primitive, `.sh` → hook, anything else falls back
/// to atom (the catch-all).
fn block_type_for(path: &Path) -> BlockType {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => BlockType::Primitive,
        Some("sh") => BlockType::Hook,
        _ => BlockType::Atom,
    }
}
