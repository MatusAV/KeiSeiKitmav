use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

/// Resolve repo root: parent of PLAN.toml's parent dir.
/// Errors loudly if canonicalize fails or parent chain breaks.
pub fn repo_root(plan_path: &Path) -> Result<PathBuf> {
    let abs = std::fs::canonicalize(plan_path)
        .map_err(|e| anyhow!("canonicalize plan {}: {}", plan_path.display(), e))?;
    let dir = abs
        .parent()
        .ok_or_else(|| anyhow!("plan {} has no parent dir", abs.display()))?;
    let root = dir
        .parent()
        .ok_or_else(|| anyhow!("plan dir {} has no parent (cannot resolve repo root)", dir.display()))?;
    Ok(root.to_path_buf())
}

/// Resolve a claim-relative path against `root`. Absolute paths pass through.
pub fn resolve(input: &Path, root: &Path) -> PathBuf {
    if input.is_absolute() {
        input.to_path_buf()
    } else {
        root.join(input)
    }
}

/// Confine `out` such that its canonicalized parent stays within `root`.
/// Used by render/plan to prevent --out path traversal.
pub fn confine_out(out: &Path, root: &Path) -> Result<()> {
    let root_canon = std::fs::canonicalize(root)
        .map_err(|e| anyhow!("canonicalize root {}: {}", root.display(), e))?;
    let parent = out
        .parent()
        .ok_or_else(|| anyhow!("--out {} has no parent", out.display()))?;
    if !parent.exists() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow!("create_dir_all {}: {}", parent.display(), e))?;
    }
    let parent_canon = std::fs::canonicalize(parent)
        .map_err(|e| anyhow!("canonicalize --out parent {}: {}", parent.display(), e))?;
    if !parent_canon.starts_with(&root_canon) {
        return Err(anyhow!(
            "--out {} escapes repo root {}",
            parent_canon.display(),
            root_canon.display()
        ));
    }
    Ok(())
}
