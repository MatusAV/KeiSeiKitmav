//! Walk the workspace for `[package.metadata.keisei.formula]` declarations
//! in member `Cargo.toml` files.
//!
//! Each declaration yields a `FormulaDecl`: the crate path (relative to
//! workspace root), the package name, the parsed effects list, and the
//! list of declared invariants. Used by `emit::derive_plan` to bridge a
//! kei-registry-driven derivation with hand-declared formulas.
//!
//! Constructor Pattern: this cube ONLY does discovery + parsing. No
//! projection, no emission. Returns sorted, deterministic output.

use anyhow::{Context, Result};
use kei_registry::Predicate;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// One declared formula extracted from a member `Cargo.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct FormulaDecl {
    pub package_name: String,
    pub manifest_dir: PathBuf,
    pub effects: Vec<String>,
    pub invariants: Vec<Predicate>,
}

#[derive(Deserialize)]
struct ManifestRoot {
    package: Option<ManifestPackage>,
}

#[derive(Deserialize)]
struct ManifestPackage {
    name: String,
    metadata: Option<ManifestMetadata>,
}

#[derive(Deserialize)]
struct ManifestMetadata {
    keisei: Option<ManifestKeisei>,
}

#[derive(Deserialize)]
struct ManifestKeisei {
    formula: Option<FormulaTable>,
}

#[derive(Deserialize)]
struct FormulaTable {
    #[serde(default)]
    effects: Vec<String>,
    #[serde(default)]
    invariant: Vec<toml::Value>,
}

/// Walk `workspace_root` (depth ≤ 3) for member `Cargo.toml` files and
/// extract any `[package.metadata.keisei.formula]` declarations. Output is
/// sorted by package name for determinism.
pub fn discover_formulas(workspace_root: &Path) -> Result<Vec<FormulaDecl>> {
    let mut out = Vec::new();
    for entry in WalkDir::new(workspace_root)
        .max_depth(4)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !is_cargo_toml(entry.path()) {
            continue;
        }
        if let Some(decl) = parse_one(entry.path(), workspace_root)? {
            out.push(decl);
        }
    }
    out.sort_by(|a, b| a.package_name.cmp(&b.package_name));
    Ok(out)
}

fn is_cargo_toml(p: &Path) -> bool {
    p.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml")
}

fn parse_one(manifest: &Path, workspace_root: &Path) -> Result<Option<FormulaDecl>> {
    let bytes = match std::fs::read_to_string(manifest) {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };
    let root: ManifestRoot = match toml::from_str(&bytes) {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };
    let Some(pkg) = root.package else {
        return Ok(None);
    };
    let Some(formula) = pkg.metadata.and_then(|m| m.keisei).and_then(|k| k.formula) else {
        return Ok(None);
    };
    let manifest_dir = manifest
        .parent()
        .unwrap_or(workspace_root)
        .strip_prefix(workspace_root)
        .unwrap_or(manifest.parent().unwrap_or(workspace_root))
        .to_path_buf();
    let invariants = parse_invariants(&formula.invariant)?;
    Ok(Some(FormulaDecl {
        package_name: pkg.name,
        manifest_dir,
        effects: formula.effects,
        invariants,
    }))
}

fn parse_invariants(values: &[toml::Value]) -> Result<Vec<Predicate>> {
    let mut out = Vec::with_capacity(values.len());
    for (i, v) in values.iter().enumerate() {
        let json = serde_json::to_string(v)
            .with_context(|| format!("invariant[{}]: convert to json", i))?;
        let pred: Predicate = serde_json::from_str(&json)
            .with_context(|| format!("invariant[{}]: parse predicate from {}", i, json))?;
        out.push(pred);
    }
    Ok(out)
}

/// Walk `workspace_root` for the canonical block-source files used by the
/// PR-4 inference pass: every `_primitives/_rust/*/src/lib.rs`,
/// `_primitives/_rust/*/src/main.rs`, plus every `hooks/*.sh`. Output is
/// sorted by path for deterministic inference order.
pub fn walk_blocks(workspace_root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    collect_rust_block_sources(workspace_root, &mut out);
    collect_hook_scripts(workspace_root, &mut out);
    out.sort();
    Ok(out)
}

fn collect_rust_block_sources(workspace_root: &Path, out: &mut Vec<PathBuf>) {
    let primitives = workspace_root.join("_primitives").join("_rust");
    if !primitives.is_dir() {
        return;
    }
    let crates = match std::fs::read_dir(&primitives) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for crate_entry in crates.flatten() {
        let src = crate_entry.path().join("src");
        for fname in &["lib.rs", "main.rs"] {
            let candidate = src.join(fname);
            if candidate.is_file() {
                out.push(candidate);
            }
        }
    }
}

fn collect_hook_scripts(workspace_root: &Path, out: &mut Vec<PathBuf>) {
    let hooks = workspace_root.join("hooks");
    if !hooks.is_dir() {
        return;
    }
    for entry in WalkDir::new(&hooks).max_depth(1).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("sh") {
            out.push(path.to_path_buf());
        }
    }
}
