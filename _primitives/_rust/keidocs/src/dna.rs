//! DNA hash — sha256 of (path + sorted deps + content), truncated 16 hex chars.
//!
//! Stable: same inputs → same hash. Sorting deps removes spurious diff noise.

use sha2::{Digest, Sha256};

/// Compute a deterministic content-addressable id for a source file.
///
/// Format: `sha256:<16-hex-chars>` — first 64 bits of the digest.
/// Inputs are hashed with explicit field separators so a path collision
/// with a dep name cannot produce the same digest.
pub fn compute_dna(file_path: &str, content: &str, deps: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"path:");
    hasher.update(file_path.as_bytes());
    hasher.update(b"\n");

    let mut sorted: Vec<&String> = deps.iter().collect();
    sorted.sort();
    hasher.update(b"deps:");
    for d in sorted {
        hasher.update(d.as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"\n");

    hasher.update(b"content:");
    hasher.update(content.as_bytes());

    let digest = hasher.finalize();
    let hex: String = digest
        .iter()
        .take(8)
        .map(|b| format!("{:02x}", b))
        .collect();
    format!("sha256:{}", hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dna_is_stable_for_same_inputs() {
        let a = compute_dna("foo.rs", "fn x(){}", &["dep1".into(), "dep2".into()]);
        let b = compute_dna("foo.rs", "fn x(){}", &["dep1".into(), "dep2".into()]);
        assert_eq!(a, b);
    }

    #[test]
    fn dna_changes_on_content_change() {
        let a = compute_dna("foo.rs", "fn x(){}", &[]);
        let b = compute_dna("foo.rs", "fn y(){}", &[]);
        assert_ne!(a, b);
    }

    #[test]
    fn dna_independent_of_deps_order() {
        let a = compute_dna("f.rs", "x", &["a".into(), "b".into()]);
        let b = compute_dna("f.rs", "x", &["b".into(), "a".into()]);
        assert_eq!(a, b);
    }

    #[test]
    fn dna_format_matches_prefix() {
        let h = compute_dna("a", "b", &[]);
        assert!(h.starts_with("sha256:"));
        assert_eq!(h.len(), "sha256:".len() + 16);
    }
}
