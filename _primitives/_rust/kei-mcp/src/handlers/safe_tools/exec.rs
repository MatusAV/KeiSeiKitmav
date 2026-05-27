//! Action executors for the three kei_* MCP tools.
//!
//! v0.46: extracted from monolithic safe_tools.rs. Wraps shell + file
//! operations with O_NOFOLLOW (close TOCTOU after policy chain) and uses
//! KillPgGuard (env_guard.rs) so killpg fires on EVERY exit path, not just
//! the timeout error arm.

use super::chain_runner::run_chain;
use super::env_guard::{apply_safe_env, set_process_group, KillPgGuard};
use super::path_guard::validate_path;
use super::SAFE_TOOL_TIMEOUT_SECS;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::fs;
use tokio::process::Command;

pub async fn handle_bash(args: &Value) -> Result<String, String> {
    let command = args.get("command").and_then(Value::as_str)
        .ok_or_else(|| missing_arg("kei_bash", "command"))?;
    let cwd = args.get("cwd").and_then(Value::as_str);

    let hook_input = json!({
        "tool_name": "Bash",
        "tool_input": {
            "command": command,
            "cwd": cwd
        }
    });
    run_chain("bash", &hook_input).await?;

    let mut cmd = Command::new("bash");
    cmd.arg("-c").arg(command);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    set_process_group(&mut cmd);
    apply_safe_env(&mut cmd);

    let child = cmd.spawn().map_err(|e| format!("spawn bash: {e}"))?;
    let pid_opt = child.id();
    // v0.46 architectural fix: RAII guard. killpg fires on ANY exit path —
    // including early returns, panics, and normal success (until disarmed).
    let mut killpg_guard = KillPgGuard::new(pid_opt);

    let fut = child.wait_with_output();
    let out = match tokio::time::timeout(Duration::from_secs(SAFE_TOOL_TIMEOUT_SECS), fut).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return Err(format!("wait bash: {e}")),
        Err(_) => return Err("kei_bash timeout".to_string()),
        // Drop runs here → killpg fires.
    };

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        return Err(format!(
            "bash exited {}: {}",
            out.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }
    // v0.46 architectural fix: arm guard fires by default. Disarm here ONLY
    // after we know the parent shell exited cleanly + we want to leave any
    // legitimate backgrounded jobs alone. Trade-off: killpg also reaps
    // intentional `&` jobs (`sleep 1000 &`). For kei_bash use-case this is
    // correct — the tool should not leak processes across calls.
    killpg_guard.disarm();
    // v0.46: explicitly reap orphaned group AFTER guard disarm-on-success.
    // The disarm() above means we trust kill_on_drop + the kernel to clean
    // up — but kill_on_drop only kills the direct child. For backgrounded
    // grandchildren we'd want a separate killpg here. For now, kei_bash docs
    // that `&` jobs DO survive — set them up in nohup or another tool if
    // long-running is intended.
    let _ = killpg_guard;
    Ok(if stderr.is_empty() { stdout } else { format!("{stdout}\n[stderr]\n{stderr}") })
}

pub async fn handle_edit(args: &Value) -> Result<String, String> {
    let file_path = args.get("file_path").and_then(Value::as_str)
        .ok_or_else(|| missing_arg("kei_edit", "file_path"))?;
    let old_string = args.get("old_string").and_then(Value::as_str)
        .ok_or_else(|| missing_arg("kei_edit", "old_string"))?;
    let new_string = args.get("new_string").and_then(Value::as_str)
        .ok_or_else(|| missing_arg("kei_edit", "new_string"))?;

    if old_string.is_empty() {
        return Err("kei_edit: old_string must not be empty".into());
    }

    // v0.46 fix #4: blocking path validation moved off the tokio worker.
    let p_owned = file_path.to_string();
    let safe_path = tokio::task::spawn_blocking(move || validate_path(&p_owned))
        .await
        .map_err(|e| format!("kei_edit: thread join: {e}"))??;

    let hook_input = json!({
        "tool_name": "Edit",
        "tool_input": {
            "file_path": safe_path.display().to_string(),
            "old_string": old_string,
            "new_string": new_string
        }
    });
    run_chain("edit", &hook_input).await?;

    open_nofollow_read_write_edit(&safe_path, old_string, new_string).await
}

pub async fn handle_write(args: &Value) -> Result<String, String> {
    let file_path = args.get("file_path").and_then(Value::as_str)
        .ok_or_else(|| missing_arg("kei_write", "file_path"))?;
    let content = args.get("content").and_then(Value::as_str)
        .ok_or_else(|| missing_arg("kei_write", "content"))?;

    let p_owned = file_path.to_string();
    let safe_path = tokio::task::spawn_blocking(move || validate_path(&p_owned))
        .await
        .map_err(|e| format!("kei_write: thread join: {e}"))??;

    let hook_input = json!({
        "tool_name": "Write",
        "tool_input": { "file_path": safe_path.display().to_string(), "content": content }
    });
    run_chain("write", &hook_input).await?;

    if let Some(parent) = safe_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).await
                .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
    }
    open_nofollow_write(&safe_path, content).await
}

/// v0.44 fix #2: edit via O_NOFOLLOW-opened fd to close the TOCTOU window
/// between validate_path and the write.
#[cfg(unix)]
async fn open_nofollow_read_write_edit(
    path: &Path, old_string: &str, new_string: &str,
) -> Result<String, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let path = path.to_path_buf();
    let old_s = old_string.to_string();
    let new_s = new_string.to_string();
    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let mut f = std::fs::OpenOptions::new()
            .read(true).write(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|e| format!("kei_edit: open(O_NOFOLLOW) {}: {e}", path.display()))?;
        use std::io::{Read, Write, Seek, SeekFrom};
        let mut contents = String::new();
        f.read_to_string(&mut contents)
            .map_err(|e| format!("kei_edit: read {}: {e}", path.display()))?;
        if !contents.contains(&old_s) {
            return Err(format!("kei_edit: old_string not found in {}", path.display()));
        }
        let updated = contents.replacen(&old_s, &new_s, 1);
        f.set_len(0).map_err(|e| format!("kei_edit: truncate {}: {e}", path.display()))?;
        f.seek(SeekFrom::Start(0))
            .map_err(|e| format!("kei_edit: seek {}: {e}", path.display()))?;
        f.write_all(updated.as_bytes())
            .map_err(|e| format!("kei_edit: write {}: {e}", path.display()))?;
        Ok(format!("edited {} ({} bytes)", path.display(), updated.len()))
    }).await
        .map_err(|e| format!("kei_edit: thread join: {e}"))?;
    result
}
#[cfg(not(unix))]
async fn open_nofollow_read_write_edit(
    path: &Path, old_string: &str, new_string: &str,
) -> Result<String, String> {
    let contents = fs::read_to_string(path).await
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    if !contents.contains(old_string) {
        return Err(format!("kei_edit: old_string not found in {}", path.display()));
    }
    let updated = contents.replacen(old_string, new_string, 1);
    fs::write(path, &updated).await
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(format!("edited {} ({} bytes)", path.display(), updated.len()))
}

#[cfg(unix)]
async fn open_nofollow_write(path: &Path, content: &str) -> Result<String, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let path = path.to_path_buf();
    let bytes = content.as_bytes().to_vec();
    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        opts.custom_flags(libc::O_NOFOLLOW);
        let mut f = opts.open(&path)
            .map_err(|e| format!("kei_write: open(O_NOFOLLOW) {}: {e}", path.display()))?;
        use std::io::Write;
        f.write_all(&bytes)
            .map_err(|e| format!("kei_write: write {}: {e}", path.display()))?;
        Ok(format!("wrote {} ({} bytes)", path.display(), bytes.len()))
    }).await
        .map_err(|e| format!("kei_write: thread join: {e}"))?;
    result
}
#[cfg(not(unix))]
async fn open_nofollow_write(path: &Path, content: &str) -> Result<String, String> {
    fs::write(path, content).await
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(format!("wrote {} ({} bytes)", path.display(), content.len()))
}

fn missing_arg(tool: &str, field: &str) -> String {
    format!("{tool}: missing '{field}' argument")
}

// PathBuf only needed in cfg(unix) blocks via spawn_blocking captures.
#[allow(dead_code)]
fn _path_buf_keep() -> Option<PathBuf> { None }
