//! kei-registry binary entry point.
//!
//! Constructor Pattern: parse CLI args; intercept `RegisterStatusTruth`
//! locally (Phase 3 Layer 3 pipe — keeps `handlers.rs` untouched);
//! delegate everything else to `handlers::dispatch`.
//! Exit codes: 0 success, 1 IO error, 2 not-found, 3 schema mismatch.

use clap::Parser;
use kei_registry::cli::{Cli, Command};
use kei_registry::handlers::{dispatch, Outcome};
use kei_registry::status_truth;
use rusqlite::Connection;
use std::io::Read;
use std::path::{Path, PathBuf};

fn main() {
    let cli = Cli::parse();
    let exit_code = match cli.command {
        Command::RegisterStatusTruth { db, block_id, input } => {
            handle_register_status_truth(db, block_id, input)
        }
        other => match dispatch(other) {
            Ok(Outcome::Ok) => 0,
            Ok(Outcome::NotFound(target)) => {
                eprintln!("kei-registry: not found: {target}");
                2
            }
            Err(e) => {
                eprintln!("kei-registry: error: {e:#}");
                1
            }
        },
    };
    std::process::exit(exit_code);
}

fn handle_register_status_truth(
    db: Option<PathBuf>,
    block_id: String,
    input: PathBuf,
) -> i32 {
    match run_register_status_truth(db, &block_id, &input) {
        Ok(true) => {
            println!("{{\"ok\":true,\"block_id\":\"{block_id}\",\"inserted\":true}}");
            0
        }
        Ok(false) => {
            println!("{{\"ok\":true,\"block_id\":\"{block_id}\",\"inserted\":false,\"reason\":\"functional_zero_stubs\"}}");
            0
        }
        Err(e) => {
            eprintln!("kei-registry: register-status-truth: {e:#}");
            1
        }
    }
}

fn run_register_status_truth(
    db: Option<PathBuf>,
    block_id: &str,
    input: &Path,
) -> anyhow::Result<bool> {
    let text = read_input(input)?;
    let marker = status_truth::parse_marker(&text)?;
    let db_path = resolve_registry_db(db);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let conn = Connection::open(&db_path)?;
    status_truth::register(&conn, block_id, &marker)
}

fn read_input(input: &Path) -> anyhow::Result<String> {
    if input.as_os_str() == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        Ok(buf)
    } else {
        Ok(std::fs::read_to_string(input)?)
    }
}

fn resolve_registry_db(db: Option<PathBuf>) -> PathBuf {
    db.unwrap_or_else(|| {
        if let Ok(env_db) = std::env::var("KEI_REGISTRY_DB") {
            return PathBuf::from(env_db);
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(".claude").join("registry.sqlite")
    })
}
