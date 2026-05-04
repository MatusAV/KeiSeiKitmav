//! kei-arch-derive CLI — emit / coverage / infer.
//!
//! Constructor Pattern: each subcommand is a thin wrapper around one
//! library entry point. Heavy lifting lives in `lib.rs` modules.

use anyhow::Result;
use clap::{Parser, Subcommand};
use kei_arch_derive::{compute_coverage, discover_formulas, emit::derive_plan, emit_plan};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "kei-arch-derive",
    about = "Bridge: kei-registry formulas → arch/PLAN.toml (Phase 2 PR-3)"
)]
struct Cli {
    /// Path to the kei-registry SQLite database. Read-only in v0.1 (the
    /// emit path uses Cargo.toml `[package.metadata.keisei.formula]`
    /// declarations). PR-4 inference will read this for body-SHA + effect
    /// derivation against per-block bodies.
    #[arg(long, default_value = "~/.claude/agents/ledger.sqlite")]
    registry: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Emit {
        #[arg(long, default_value = "arch/PLAN.toml")]
        out: PathBuf,
        #[arg(long, default_value_t = false)]
        check_format: bool,
    },
    Coverage {
        #[arg(long, default_value_t = 0.0)]
        min_presence: f64,
        #[arg(long, default_value_t = 0.0)]
        min_agreement: f64,
    },
    Infer,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Emit { out, check_format } => run_emit(&cli.workspace, &out, check_format),
        Cmd::Coverage {
            min_presence,
            min_agreement,
        } => run_coverage(&cli.workspace, min_presence, min_agreement),
        Cmd::Infer => run_infer(&cli.workspace, cli.registry.as_str()),
    }
}

fn run_emit(workspace: &Path, out: &Path, check_format: bool) -> Result<()> {
    let decls = discover_formulas(workspace)?;
    let plan = derive_plan(&decls, "https://github.com/KeiSei84/KeiSeiKit-1.0/blob/main/");
    if check_format {
        let rendered = kei_arch_derive::render_plan_string(&plan);
        let on_disk = std::fs::read_to_string(out).unwrap_or_default();
        if rendered != on_disk {
            anyhow::bail!("emit drift: regenerate {}", out.display());
        }
        println!("[OK] {} matches derived output", out.display());
        return Ok(());
    }
    emit_plan(&plan, out)?;
    println!(
        "[OK] emitted {} ({} module(s), {} claim(s))",
        out.display(),
        plan.modules.len(),
        plan.modules.iter().map(|m| m.claims.len()).sum::<usize>()
    );
    Ok(())
}

fn run_coverage(workspace: &Path, min_presence: f64, min_agreement: f64) -> Result<()> {
    use kei_registry::EffectKind;
    use std::collections::BTreeSet;
    let decls = discover_formulas(workspace)?;
    let blocks_total = decls.len().max(1);
    let pairs: Vec<(BTreeSet<EffectKind>, BTreeSet<EffectKind>)> = decls
        .iter()
        .map(|_| (BTreeSet::new(), BTreeSet::new()))
        .collect();
    let cov = compute_coverage(blocks_total, &pairs);
    println!("{}", serde_json::to_string_pretty(&cov)?);
    if cov.presence < min_presence {
        anyhow::bail!(
            "presence {:.3} below threshold {:.3}",
            cov.presence,
            min_presence
        );
    }
    if cov.agreement < min_agreement {
        anyhow::bail!(
            "agreement {:.3} below threshold {:.3}",
            cov.agreement,
            min_agreement
        );
    }
    Ok(())
}

fn run_infer(workspace: &Path, registry_db: &str) -> Result<()> {
    let db_path = expand_registry_path(registry_db);
    let count = kei_arch_derive::infer::run(workspace, &db_path)?;
    println!(
        "[OK] inferred {} formula(s) from {} into {}",
        count,
        workspace.display(),
        db_path.display()
    );
    Ok(())
}

/// Expand a leading `~/` to the user's home directory. Returns the input
/// path verbatim when no expansion is needed.
fn expand_registry_path(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(raw)
}
