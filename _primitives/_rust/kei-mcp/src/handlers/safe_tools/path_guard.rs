//! Path-traversal + symlink + denylist guard for `kei_edit` / `kei_write`.
//!
//! v0.46: extracted from monolithic safe_tools.rs. Pure-sync helpers — the
//! async handlers in exec.rs wrap them in `spawn_blocking` so a slow
//! `canonicalize` syscall doesn't starve a tokio worker (v0.46 fix #4).

use std::path::{Path, PathBuf};

/// v0.41 (initial): rejected `..`, canonicalized PARENT, checked denylist + roots.
///   → 4-CLI re-audit (2026-05-26) found this was bypassable via symlink at the
///     leaf and self-attackable via the $HOME blanket-allowed root.
///
/// v0.42 fixes:
///   #1 [CRITICAL] reject if the leaf is a symlink for new files; canonicalize
///      full path when the file exists.
///   #2 [HIGH] $HOME removed from default allowed-roots — default is $PWD only.
///      Denylist now also covers $HOME/.claude/ (the substrate itself), shell
///      init files, and credential stores.
///
/// v0.44 fixes:
///   #1 [CRITICAL] walk_up_to_canonicalize — finds deepest existing ancestor,
///      canonicalizes THAT (resolving all symlinks in the existing prefix),
///      reattaches the non-existent tail. Closes the "parent's parent is a
///      symlink" bypass.
///   #5 [HIGH] Path::starts_with for component-aware containment + canonical
///      KEI_ALLOWED_ROOTS so /var → /private/var symlink works on macOS.
///   #6 [MED] allowed_roots check FIRST; narrowed /var/ blanket to /var/db/,
///      /var/log/, /var/root/ — macOS $TMPDIR = /var/folders/ now allowed.
pub fn validate_path(p: &str) -> Result<PathBuf, String> {
    if p.is_empty() {
        return Err("file_path: empty".into());
    }
    if p.split('/').any(|seg| seg == "..") {
        return Err(format!("file_path: '..' segment not allowed in {p}"));
    }
    let path = Path::new(p);
    let canonical = canonicalize_with_walk_up(path)?;

    // Reject if the leaf is a symlink (covers dangling symlinks for new files).
    if let Ok(meta) = std::fs::symlink_metadata(&canonical) {
        if meta.file_type().is_symlink() {
            return Err(format!(
                "file_path: leaf is a symlink (refusing to follow): {}",
                canonical.display()
            ));
        }
    }

    // Allowed-root containment FIRST (v0.44 fix #6).
    let roots = allowed_roots();
    // v0.46 fix #3: empty allowed_roots → fail-CLOSED (was: silently
    // disabled containment). Operator must explicitly set KEI_ALLOWED_ROOTS
    // to "" if they want to disable, and we still reject empty.
    if roots.is_empty() {
        return Err(
            "file_path: allowed_roots is empty — refusing all writes \
             (set KEI_ALLOWED_ROOTS to a non-empty value or run from a real cwd)".into()
        );
    }
    let in_allowed_root = roots.iter().any(|r| canonical.starts_with(r));
    if !in_allowed_root {
        return Err(format!(
            "file_path: outside allowed roots {:?}: {}",
            roots, canonical.display()
        ));
    }

    let canon_str = canonical.display().to_string();

    // Reject system + substrate-control + credential paths.
    let denylist = [
        "/etc/", "/usr/", "/System/", "/var/db/", "/var/log/", "/var/root/",
        "/private/etc/", "/private/var/db/", "/private/var/log/", "/private/var/root/",
        "/root/", "/bin/", "/sbin/",
    ];
    for d in denylist {
        if canon_str.starts_with(d) {
            return Err(format!("file_path: denied (system dir): {canon_str}"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let dir_secrets = [
            ".ssh/", ".aws/", ".gnupg/", ".config/gcloud/", ".cargo/credentials",
            ".npmrc", ".docker/config.json", ".kube/",
            ".claude/", ".grok/", ".gemini/", ".copilot/", ".kimi/",
        ];
        for sd in dir_secrets {
            let full = format!("{home}/{sd}");
            if canon_str.starts_with(&full) {
                return Err(format!("file_path: denied (secret/substrate dir): {canon_str}"));
            }
        }
        let init_files = [
            ".zshrc", ".bashrc", ".profile", ".bash_profile", ".zprofile",
            ".zshenv", ".bash_login", ".inputrc", ".gitconfig",
            ".config/fish/config.fish",
        ];
        for f in init_files {
            let full = format!("{home}/{f}");
            if canon_str == full {
                return Err(format!("file_path: denied (shell-init file): {canon_str}"));
            }
        }
    }

    Ok(canonical)
}

/// v0.44 fix #1: walk up the path looking for the deepest existing ancestor,
/// canonicalize THAT, then reattach the non-existent tail components.
fn canonicalize_with_walk_up(path: &Path) -> Result<PathBuf, String> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("file_path: cwd unavailable: {e}"))?
            .join(path)
    };

    let mut current = abs.clone();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let canon = loop {
        if current.exists() {
            break current.canonicalize()
                .map_err(|e| format!("file_path: canonicalize {}: {e}", current.display()))?;
        }
        let name = current.file_name()
            .ok_or_else(|| format!("file_path: path has no existing ancestor: {}", abs.display()))?
            .to_os_string();
        let parent = match current.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => return Err(format!("file_path: walked to root without finding existing dir: {}", abs.display())),
        };
        tail.push(name);
        current = parent;
    };

    let mut result = canon;
    for name in tail.into_iter().rev() {
        result.push(name);
    }
    Ok(result)
}

pub fn allowed_roots() -> Vec<String> {
    let canon_with_slash = |raw: &str| -> Option<String> {
        let p = Path::new(raw);
        let canon = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let mut s = canon.display().to_string();
        if !s.ends_with('/') { s.push('/'); }
        if s.is_empty() { None } else { Some(s) }
    };
    if let Ok(v) = std::env::var("KEI_ALLOWED_ROOTS") {
        return v.split(':')
            .filter(|s| !s.is_empty())
            .filter_map(canon_with_slash)
            .collect();
    }
    let mut roots = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(r) = canon_with_slash(&cwd.display().to_string()) {
            roots.push(r);
        }
    }
    roots
}
