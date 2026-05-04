//! DNA manifest schema + compute / read / write / verify.
//!
//! Aggregate `dna_hash` is sha256 over a canonical concatenation of:
//!   name | version | sorted(file_path:sha256) | sorted(deps)
//! Order-independent in deps; deterministic across machines.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Lineage {
    pub parent_dna: Option<String>,
    pub fork_of: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DnaManifest {
    pub name: String,
    pub version: String,
    pub dna_hash: String,
    pub files: Vec<FileEntry>,
    pub deps: Vec<String>,
    pub generated: String,
    pub git_commit: String,
    pub author: String,
    pub lineage: Lineage,
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

fn collect_source_files(root: &Path) -> Result<Vec<FileEntry>> {
    let mut out: Vec<FileEntry> = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let rel = p.strip_prefix(root).unwrap_or(p);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        // Cargo.toml NOT included — deps already tracked separately in deps[].
        // Including Cargo.toml hash here breaks dep_order_is_normalized invariant
        // since Cargo.toml text differs by dep ordering even if normalized deps match.
        let included = rel_str.starts_with("src/") && rel_str.ends_with(".rs");
        if !included {
            continue;
        }
        let bytes = fs::read(p)
            .with_context(|| format!("read {}", p.display()))?;
        out.push(FileEntry { path: rel_str, sha256: sha256_hex(&bytes) });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn ingest_line(t: &str, in_deps: bool, name: &mut String, version: &mut String, deps: &mut Vec<String>) {
    if let Some(rest) = t.strip_prefix("name = \"") {
        if name.is_empty() { *name = rest.trim_end_matches('"').to_string(); }
    } else if let Some(rest) = t.strip_prefix("version = \"") {
        if version == "0.0.0" { *version = rest.trim_end_matches('"').to_string(); }
    } else if in_deps && !t.is_empty() && !t.starts_with('#') {
        if let Some((k, _)) = t.split_once('=') {
            deps.push(k.trim().to_string());
        }
    }
}

fn parse_cargo_toml(root: &Path) -> Result<(String, String, Vec<String>)> {
    let path = root.join("Cargo.toml");
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("read {}", path.display()))?;
    let mut name = String::new();
    let mut version = "0.0.0".to_string();
    let mut deps: Vec<String> = Vec::new();
    let mut in_deps = false;
    for line in raw.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_deps = t == "[dependencies]";
            continue;
        }
        ingest_line(t, in_deps, &mut name, &mut version, &mut deps);
    }
    deps.sort();
    deps.dedup();
    if name.is_empty() {
        return Err(anyhow!("Cargo.toml missing [package] name"));
    }
    Ok((name, version, deps))
}

fn aggregate_dna_hash(
    name: &str,
    version: &str,
    files: &[FileEntry],
    deps: &[String],
) -> String {
    let mut h = Sha256::new();
    h.update(name.as_bytes());
    h.update(b"|");
    h.update(version.as_bytes());
    h.update(b"|");
    for f in files {
        h.update(f.path.as_bytes());
        h.update(b":");
        h.update(f.sha256.as_bytes());
        h.update(b"\n");
    }
    h.update(b"|");
    for d in deps {
        h.update(d.as_bytes());
        h.update(b",");
    }
    format!("sha256:{}", hex::encode(h.finalize()))
}

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| fallback.to_string())
}

pub fn compute_primitive_dna(root: &Path) -> Result<DnaManifest> {
    let (name, version, deps) = parse_cargo_toml(root)?;
    let files = collect_source_files(root)?;
    let dna_hash = aggregate_dna_hash(&name, &version, &files, &deps);
    let generated = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    Ok(DnaManifest {
        name,
        version,
        dna_hash,
        files,
        deps,
        generated,
        git_commit: env_or("GIT_COMMIT", "unknown"),
        author: env_or("GIT_AUTHOR", "unknown"),
        lineage: Lineage { parent_dna: None, fork_of: None },
    })
}

pub fn write_to(path: &Path, manifest: &DnaManifest) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let s = serde_json::to_string_pretty(manifest)
        .context("serialize manifest")?;
    fs::write(path, s).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn read_from(path: &Path) -> Result<DnaManifest> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    let m: DnaManifest = serde_json::from_str(&raw)
        .with_context(|| format!("parse manifest {}", path.display()))?;
    Ok(m)
}

pub fn verify(manifest: &DnaManifest, root: &Path) -> Result<bool> {
    let fresh = compute_primitive_dna(root)?;
    Ok(fresh.dna_hash == manifest.dna_hash
        && fresh.files == manifest.files
        && fresh.deps == manifest.deps)
}

pub fn dna_path(primitive_root: &Path) -> PathBuf {
    primitive_root.join(".dna.json")
}
