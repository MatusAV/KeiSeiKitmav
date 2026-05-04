//! keidna-sign CLI — emit / verify / list DNA manifests.

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use keidna_sign::{
    compute_primitive_dna, dna_path, read_from, verify as verify_manifest, write_to,
};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "keidna-sign", about = "DNA manifest for KeiSeiKit primitives")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Emit `.dna.json` for a primitive.
    Emit {
        #[arg(long)]
        primitive: String,
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Verify an existing `.dna.json` matches current source.
    Verify {
        #[arg(long)]
        primitive: String,
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// List all primitives + their DNA hashes (table).
    List {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

fn primitive_root(workspace_root: &Path, name: &str) -> PathBuf {
    workspace_root.join(name)
}

fn cmd_emit(workspace_root: &Path, primitive: &str) -> Result<()> {
    let proot = primitive_root(workspace_root, primitive);
    if !proot.is_dir() {
        return Err(anyhow!("primitive directory not found: {}", proot.display()));
    }
    let manifest = compute_primitive_dna(&proot)
        .with_context(|| format!("compute DNA for {}", primitive))?;
    let out = dna_path(&proot);
    write_to(&out, &manifest)?;
    println!("emitted {}", out.display());
    println!("  dna_hash = {}", manifest.dna_hash);
    println!("  files    = {}", manifest.files.len());
    println!("  deps     = {}", manifest.deps.len());
    Ok(())
}

fn cmd_verify(workspace_root: &Path, primitive: &str) -> Result<()> {
    let proot = primitive_root(workspace_root, primitive);
    let mpath = dna_path(&proot);
    if !mpath.is_file() {
        return Err(anyhow!("no .dna.json at {}", mpath.display()));
    }
    let stored = read_from(&mpath)?;
    let ok = verify_manifest(&stored, &proot)?;
    if ok {
        println!("OK {} {}", primitive, stored.dna_hash);
        Ok(())
    } else {
        Err(anyhow!("DNA mismatch for {} (source changed since emit)", primitive))
    }
}

fn list_primitive_dirs(workspace_root: &Path) -> Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(workspace_root)
        .with_context(|| format!("read_dir {}", workspace_root.display()))?
    {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() && p.join("Cargo.toml").is_file() {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

fn cmd_list(workspace_root: &Path) -> Result<()> {
    let dirs = list_primitive_dirs(workspace_root)?;
    println!("{:<40} {:<22} {}", "PRIMITIVE", "VERSION", "DNA");
    for d in dirs {
        let mpath = dna_path(&d);
        let name = d.file_name().and_then(|s| s.to_str()).unwrap_or("?");
        if let Ok(m) = read_from(&mpath) {
            println!("{:<40} {:<22} {}", name, m.version, m.dna_hash);
        } else {
            println!("{:<40} {:<22} (no .dna.json)", name, "-");
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Emit { primitive, root } => cmd_emit(&root, &primitive),
        Cmd::Verify { primitive, root } => cmd_verify(&root, &primitive),
        Cmd::List { root } => cmd_list(&root),
    }
}
