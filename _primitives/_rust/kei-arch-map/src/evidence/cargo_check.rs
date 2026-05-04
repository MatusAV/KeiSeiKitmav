use super::path_resolve;
use serde::Deserialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;
use wait_timeout::ChildExt;

const TIMEOUT: Duration = Duration::from_secs(300);

/// Cached cargo binary path. `which` is invoked once per process; subsequent
/// calls reuse the cached `Result`. Cloning is cheap (PathBuf is Arc-free
/// but the surrounding Result is small).
static CARGO_PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();

/// Resolve the `cargo` binary to an ABSOLUTE path via the `which` crate.
/// #50B PATH-hijack guard: never invoke a bare "cargo" — a malicious
/// PATH entry could shadow the real binary. Reject relative paths and
/// non-existent results. Public so integration tests can verify the
/// guard's invariant (absolute + exists).
pub fn resolve_cargo() -> Result<PathBuf, String> {
    CARGO_PATH
        .get_or_init(|| {
            which::which("cargo")
                .map_err(|e| format!("cargo binary not in PATH: {}", e))
                .and_then(|p| {
                    if !p.is_absolute() {
                        return Err(format!("cargo path not absolute: {}", p.display()));
                    }
                    if !p.exists() {
                        return Err(format!("cargo path does not exist: {}", p.display()));
                    }
                    Ok(p)
                })
        })
        .clone()
}

/// Captured cargo output. Mirrors `std::process::Output` but built from
/// background-drained pipes so the child cannot deadlock on a full
/// 64 KiB pipe buffer when JSON output exceeds it.
struct DrainedOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct Diag {
    reason: String,
    #[serde(default)]
    message: Option<DiagMessage>,
}

#[derive(Debug, Deserialize)]
struct DiagMessage {
    #[serde(default)]
    level: String,
}

/// Spawn cargo check with fixed argv (NOT through sh).
/// #50B: cargo binary is resolved to an absolute path via `which` so a
/// PATH-hijack cannot redirect to an attacker binary.
fn spawn(dir: &Path) -> Result<std::process::Child, String> {
    let cargo = resolve_cargo()?;
    Command::new(&cargo)
        .args([
            "check",
            "--workspace",
            "--offline",
            "--message-format=json",
        ])
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn cargo: {}", e))
}

/// Spawn a thread that reads a pipe to EOF into a Vec<u8>.
fn drain<R: Read + Send + 'static>(mut r: R) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = r.read_to_end(&mut buf);
        buf
    })
}

/// Wait with timeout while concurrently draining stdout+stderr in
/// background threads. Without the drains, cargo's JSON stream fills the
/// 64 KiB pipe buffer on a large workspace and the child blocks on write,
/// causing a false timeout.
fn wait_capped(mut child: std::process::Child) -> Result<DrainedOutput, String> {
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let stdout_h = drain(stdout);
    let stderr_h = drain(stderr);
    let res = child.wait_timeout(TIMEOUT);
    match res {
        Ok(Some(status)) => {
            let out = stdout_h.join().unwrap_or_default();
            let err = stderr_h.join().unwrap_or_default();
            Ok(DrainedOutput { status, stdout: out, stderr: err })
        }
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_h.join();
            let _ = stderr_h.join();
            Err(format!("cargo check exceeded {}s", TIMEOUT.as_secs()))
        }
        Err(e) => Err(format!("wait_timeout: {}", e)),
    }
}

/// Count compiler-error diagnostics in cargo JSON stream.
fn count_errors(stdout: &[u8]) -> u64 {
    let s = String::from_utf8_lossy(stdout);
    let mut errs = 0u64;
    for line in s.lines() {
        if line.is_empty() {
            continue;
        }
        if let Ok(d) = serde_json::from_str::<Diag>(line) {
            if d.reason == "compiler-message" {
                if let Some(m) = d.message {
                    if m.level == "error" {
                        errs += 1;
                    }
                }
            }
        }
    }
    errs
}

pub fn check(manifest_dir: &Path, root: &Path) -> (bool, String) {
    // Wave 7B path-confine: `cargo check` runs build.rs, so a malicious
    // PLAN.toml that escapes the repo via `..` or absolute path injection
    // would be an RCE. Resolution proceeds in two stages:
    //   1) ambient resolve (no canonicalize) so a missing dir gets the
    //      friendly "no Cargo.toml at <path>" reason, not a canonicalize
    //      ENOENT — preserves test contract.
    //   2) canonicalize-then-confine on the dir if it exists.
    let resolved_ambient = path_resolve::resolve(manifest_dir, root);
    if !resolved_ambient.join("Cargo.toml").exists() {
        return (
            false,
            format!("no Cargo.toml at {}", resolved_ambient.display()),
        );
    }
    let resolved = match path_resolve::resolve_confined(manifest_dir, root) {
        Ok(p) => p,
        Err(e) => return (false, e),
    };
    let child = match spawn(&resolved) {
        Ok(c) => c,
        Err(e) => return (false, e),
    };
    let out = match wait_capped(child) {
        Ok(o) => o,
        Err(e) => return (false, e),
    };
    let errs = count_errors(&out.stdout);
    if errs == 0 && out.status.success() {
        return (true, String::new());
    }
    if errs == 0 && !out.status.success() {
        let stderr_tail: String = String::from_utf8_lossy(&out.stderr).chars().take(200).collect();
        return (
            false,
            format!("cargo non-zero exit; stderr: {}", stderr_tail),
        );
    }
    (false, format!("cargo check: {} compiler-error(s)", errs))
}
