//! Roundtrip + change-detection + dep-order tests.

use keidna_sign::{compute_primitive_dna, dna_path, read_from, verify, write_to};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn write_primitive(root: &Path, name: &str, deps: &[(&str, &str)], src: &str) {
    fs::create_dir_all(root.join("src")).unwrap();
    let mut cargo = String::from("[package]\n");
    cargo.push_str(&format!("name = \"{}\"\n", name));
    cargo.push_str("version = \"0.1.0\"\n\n[dependencies]\n");
    for (k, v) in deps {
        cargo.push_str(&format!("{} = \"{}\"\n", k, v));
    }
    fs::write(root.join("Cargo.toml"), cargo).unwrap();
    fs::write(root.join("src/main.rs"), src).unwrap();
}

#[test]
fn emit_verify_roundtrip_is_deterministic() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_primitive(root, "foo", &[("anyhow", "1")], "fn main() {}\n");
    let m1 = compute_primitive_dna(root).unwrap();
    let out = dna_path(root);
    write_to(&out, &m1).unwrap();
    let stored = read_from(&out).unwrap();
    assert_eq!(stored.dna_hash, m1.dna_hash);
    assert_eq!(stored.name, "foo");
    assert!(stored.dna_hash.starts_with("sha256:"));
    assert!(verify(&stored, root).unwrap(), "fresh verify must pass");
}

#[test]
fn file_change_breaks_verify() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_primitive(root, "bar", &[("serde", "1")], "fn main() {}\n");
    let m = compute_primitive_dna(root).unwrap();
    let out = dna_path(root);
    write_to(&out, &m).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() { println!(\"x\"); }\n").unwrap();
    let stored = read_from(&out).unwrap();
    assert!(!verify(&stored, root).unwrap(), "modified source must fail verify");
}

#[test]
fn dep_order_is_normalized() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    write_primitive(dir1.path(), "baz", &[("a", "1"), ("b", "1"), ("c", "1")], "fn main(){}\n");
    write_primitive(dir2.path(), "baz", &[("c", "1"), ("a", "1"), ("b", "1")], "fn main(){}\n");
    let m1 = compute_primitive_dna(dir1.path()).unwrap();
    let m2 = compute_primitive_dna(dir2.path()).unwrap();
    assert_eq!(m1.dna_hash, m2.dna_hash, "dep order must not affect hash");
    assert_eq!(m1.deps, m2.deps);
}
