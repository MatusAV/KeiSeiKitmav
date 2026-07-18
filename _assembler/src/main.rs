//! CLI entry: build [--help] [--validate] [--dry-run] [--in-place] [<manifest.toml> ...]
//!
//! Default: read all _manifests/*.toml, write to _generated/*.md.
//! --help: print usage and exit 0. No filesystem access beyond argv.
//! --in-place: write to agents/<name>.md (replaces generated file).
//! --validate: parse + validate only, no output.
//! --dry-run: parse + validate + render, but do not write; prints what
//!   would be written instead. Unlike an unrecognized flag (previously
//!   silently dropped, falling through to a real write), this is a
//!   guaranteed no-filesystem-write path.
//! Positional args: specific manifest files to process.

mod assembler;
mod manifest;
mod placeholders;
mod registry_client;
mod schemas_export;
mod substrate;
mod validator;

use manifest::Manifest;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{env, fs};

const USAGE: &str = "\
assemble — KeiSeiKit agent manifest assembler

USAGE:
    assemble [FLAGS] [MANIFEST...]

FLAGS:
    --help, -h    Print this help and exit 0. No filesystem access.
    --validate    Parse + validate manifests only. Nothing is written.
    --dry-run     Parse, validate, and render, but do not write files.
                  Prints the path and size of what would be written.
    --in-place    Write to agents/<name>.md (the live-served manifest)
                  instead of the default _generated/<name>.md cache.

ARGS:
    MANIFEST...   Specific _manifests/*.toml files to process.
                  Default: every manifest under _manifests/.

Running `assemble` with no flags performs a REAL WRITE of every manifest
to _generated/*.md (or agents/*.md with --in-place). Use --validate or
--dry-run first if you just want to check what a build would do.
";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let root = root_dir();
    let blocks = root.join("_blocks");
    let manifests = root.join("_manifests");
    let generated = root.join("_generated");

    let validate_only = args.iter().any(|a| a == "--validate");
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let in_place = args.iter().any(|a| a == "--in-place");
    let targets: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();

    let paths: Vec<PathBuf> = if targets.is_empty() {
        collect_manifests(&manifests)
    } else {
        targets.iter().map(|t| PathBuf::from(t)).collect()
    };

    if paths.is_empty() {
        eprintln!("no manifests found in {}", manifests.display());
        return ExitCode::from(1);
    }

    let mut errors = 0u32;
    for path in &paths {
        match process(path, &blocks, &generated, &root, validate_only, dry_run, in_place) {
            Ok(Outcome::Skipped) => {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                println!("OK  {name}");
            }
            Ok(Outcome::Written(p)) => {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                println!("OK  {name} → {}", relative_to(&p, root.parent().unwrap_or(root.as_path())));
            }
            Ok(Outcome::WouldWrite(p, bytes)) => {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                println!(
                    "DRY-RUN OK  {name} → {} ({bytes} bytes, not written)",
                    relative_to(&p, root.parent().unwrap_or(root.as_path()))
                );
            }
            Err(e) => {
                eprintln!("FAIL {}: {e}", path.display());
                errors += 1;
            }
        }
    }

    if errors > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS }
}

enum Outcome {
    Skipped,
    Written(PathBuf),
    WouldWrite(PathBuf, usize),
}

fn process(
    path: &Path,
    blocks: &Path,
    generated: &Path,
    root: &Path,
    validate_only: bool,
    dry_run: bool,
    in_place: bool,
) -> Result<Outcome, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
    let m: Manifest = toml::from_str(&text).map_err(|e| format!("parse: {e}"))?;
    validator::validate(&m, blocks)?;

    if validate_only {
        return Ok(Outcome::Skipped);
    }

    let content = assembler::assemble(&m, blocks)?;
    let out_path = if in_place {
        root.join(format!("{}.md", m.name))
    } else {
        generated.join(format!("{}.md", m.name))
    };

    if dry_run {
        return Ok(Outcome::WouldWrite(out_path, content.len()));
    }

    if !in_place {
        fs::create_dir_all(generated).map_err(|e| format!("mkdir generated: {e}"))?;
    }
    fs::write(&out_path, content).map_err(|e| format!("write {}: {e}", out_path.display()))?;
    Ok(Outcome::Written(out_path))
}

fn root_dir() -> PathBuf {
    // Priority: AGENT_ROOT env > HOME/.claude/agents default.
    // (exe-relative would break when the binary is symlinked or copied.)
    if let Ok(v) = env::var("AGENT_ROOT") {
        return PathBuf::from(v);
    }
    PathBuf::from(env::var("HOME").unwrap_or_default()).join(".claude/agents")
}

fn collect_manifests(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("toml") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn relative_to(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}
