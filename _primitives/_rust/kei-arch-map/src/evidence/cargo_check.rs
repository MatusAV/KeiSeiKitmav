use super::path_resolve;
use serde::Deserialize;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

const TIMEOUT: Duration = Duration::from_secs(300);

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
fn spawn(dir: &Path) -> Result<std::process::Child, String> {
    Command::new("cargo")
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

/// Wait with timeout. On expiry, kill the child and return Err.
fn wait_capped(mut child: std::process::Child) -> Result<std::process::Output, String> {
    match child.wait_timeout(TIMEOUT) {
        Ok(Some(_status)) => child
            .wait_with_output()
            .map_err(|e| format!("wait_with_output: {}", e)),
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
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
    let resolved = path_resolve::resolve(manifest_dir, root);
    if !resolved.join("Cargo.toml").exists() {
        return (
            false,
            format!("no Cargo.toml at {}", resolved.display()),
        );
    }
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
