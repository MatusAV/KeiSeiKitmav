//! PLAN.toml serializer — deterministic, sorted, inline-evidence form.
//!
//! Modules sorted by `id`. Claims within a module sorted by `id`. Each
//! claim's evidence is rendered as an inline TOML table to match the
//! shape of the hand-written `arch/PLAN.toml` shipped in Phase 1.
//!
//! Constructor Pattern: pure projection from `DerivedPlan` to a string.
//! Inline-evidence rendering lives in `serialize.rs`. The atomic-write
//! helper here is the one I/O cube.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

use crate::project::EvidenceClaim;
use crate::serialize::{inline_evidence, quote};
use crate::walker::FormulaDecl;

/// Final emit-ready plan: meta header + sorted modules + sorted claims.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedPlan {
    pub schema_version: u32,
    pub repo_root: String,
    pub github_blob_base: String,
    pub modules: Vec<DerivedModule>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DerivedModule {
    pub id: String,
    pub path: PathBuf,
    pub description: Option<String>,
    pub claims: Vec<DerivedClaim>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DerivedClaim {
    pub id: String,
    pub description: String,
    pub evidence: EvidenceClaim,
}

/// Build a `DerivedPlan` from formula declarations. v0.1 wiring: each
/// declaration with N invariants becomes one module with N claims; an
/// empty declaration list yields a meta-only plan (correct for an empty
/// registry per the PR-3 spec).
pub fn derive_plan(decls: &[FormulaDecl], github_blob_base: &str) -> DerivedPlan {
    let mut modules: Vec<DerivedModule> = decls.iter().map(decl_to_module).collect();
    modules.sort_by(|a, b| a.id.cmp(&b.id));
    DerivedPlan {
        schema_version: 1,
        repo_root: ".".to_string(),
        github_blob_base: github_blob_base.to_string(),
        modules,
    }
}

fn decl_to_module(decl: &FormulaDecl) -> DerivedModule {
    let mut claims: Vec<DerivedClaim> = decl
        .invariants
        .iter()
        .enumerate()
        .map(|(i, p)| DerivedClaim {
            id: format!("{}-invariant-{:03}", decl.package_name, i),
            description: format!(
                "Auto-derived from {} formula invariant #{}",
                decl.package_name, i
            ),
            evidence: crate::project::predicate_to_evidence(p),
        })
        .collect();
    claims.sort_by(|a, b| a.id.cmp(&b.id));
    DerivedModule {
        id: decl.package_name.clone(),
        path: decl.manifest_dir.clone(),
        description: Some(format!(
            "Auto-derived module from [package.metadata.keisei.formula] in {}",
            decl.package_name
        )),
        claims,
    }
}

/// Render the plan to a TOML string in the inline-evidence shape used by
/// the hand-written PLAN.toml. Output is byte-stable for a given input
/// (no clock, no env, no random ordering).
pub fn render_plan_string(plan: &DerivedPlan) -> String {
    let mut out = String::new();
    out.push_str(HEADER_COMMENT);
    out.push_str("\n\n[meta]\n");
    out.push_str(&format!("schema_version = {}\n", plan.schema_version));
    out.push_str(&format!("repo_root = {}\n", quote(&plan.repo_root)));
    out.push_str(&format!(
        "github_blob_base = {}\n",
        quote(&plan.github_blob_base)
    ));
    for module in &plan.modules {
        render_module(module, &mut out);
    }
    out
}

fn render_module(module: &DerivedModule, out: &mut String) {
    out.push_str("\n[[module]]\n");
    out.push_str(&format!("id = {}\n", quote(&module.id)));
    out.push_str(&format!(
        "path = {}\n",
        quote(&module.path.display().to_string())
    ));
    if let Some(d) = &module.description {
        out.push_str(&format!("description = {}\n", quote(d)));
    }
    for claim in &module.claims {
        render_claim(claim, out);
    }
}

fn render_claim(claim: &DerivedClaim, out: &mut String) {
    out.push_str("\n[[module.claim]]\n");
    out.push_str(&format!("id = {}\n", quote(&claim.id)));
    out.push_str(&format!("description = {}\n", quote(&claim.description)));
    out.push_str(&format!(
        "evidence = {}\n",
        inline_evidence(&claim.evidence)
    ));
}

/// Atomic write to `target`. Writes to a sibling `.<name>.tmp` then
/// renames; the rename is atomic on the same filesystem.
pub fn emit_plan(plan: &DerivedPlan, target: &Path) -> Result<()> {
    let contents = render_plan_string(plan);
    let parent = target
        .parent()
        .ok_or_else(|| anyhow!("target {} has no parent", target.display()))?;
    if !parent.as_os_str().is_empty() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create dir {}", parent.display()))?;
    }
    let fname = target
        .file_name()
        .ok_or_else(|| anyhow!("target {} has no file name", target.display()))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(fname);
    tmp_name.push(".tmp");
    let tmp = parent.join(tmp_name);
    std::fs::write(&tmp, &contents).with_context(|| format!("write tmp {}", tmp.display()))?;
    std::fs::rename(&tmp, target)
        .with_context(|| format!("rename {} -> {}", tmp.display(), target.display()))?;
    Ok(())
}

const HEADER_COMMENT: &str = "# AUTO-GENERATED by kei-arch-derive v0.1.0. Do NOT hand-edit.\n\
# Source of truth: kei-registry SQLite + [package.metadata.keisei.formula] in member Cargo.toml.\n\
# Re-emit with: cargo run -p kei-arch-derive -- emit --workspace . --out arch/PLAN.toml";
