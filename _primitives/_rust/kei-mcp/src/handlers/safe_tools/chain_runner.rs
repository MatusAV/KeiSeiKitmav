//! Policy chain loader + runner.
//!
//! v0.46: extracted from monolithic safe_tools.rs. Reads
//! `~/.claude/hooks/_lib/policy-chain.toml` to get the hook list for each
//! tool kind (bash/edit/write), pipes synthesized PreToolUse input to each
//! hook, aborts on first non-zero exit.
//!
//! v0.46 architectural fix #1 (Claude critic CRITICAL): REMOVED env-based
//! chain-skip (CLAUDECODE / GROKCODE). The skip was logically broken — it
//! assumed native PreToolUse would catch the call, but PreToolUse matchers
//! fire on tool_name="Bash"|"Edit"|"Write" and MCP tools are named
//! `kei_bash`/`kei_edit`/`kei_write`. Native hooks NEVER fire on these
//! → skip created an auth-bypass hole on Grok. Chain now ALWAYS runs.

use super::env_guard::{apply_safe_env, killpg_best_effort, set_process_group};
use super::SAFE_TOOL_TIMEOUT_SECS;
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Deserialize, Default)]
struct PolicyChain {
    #[serde(default)]
    bash: ChainSpec,
    #[serde(default)]
    edit: ChainSpec,
    #[serde(default)]
    write: ChainSpec,
}

#[derive(Deserialize, Default)]
struct ChainSpec {
    #[serde(default)]
    chain: Vec<String>,
}

/// Run the configured hook chain for `tool` ("bash"/"edit"/"write").
pub async fn run_chain(tool: &str, hook_input: &Value) -> Result<(), String> {
    let chain = load_chain(tool).await?;
    if chain.is_empty() {
        // v0.42 fix #3: empty section is the same misconfig class as missing
        // file — FAIL CLOSED with explicit opt-in.
        if env_truthy("KEI_POLICY_CHAIN_OPTIONAL") {
            return Ok(());
        }
        return Err(format!(
            "[policy-chain] section [{tool}] is empty — refusing to run \
             (set KEI_POLICY_CHAIN_OPTIONAL=1 to allow pass-through, e.g. for tests)"
        ));
    }

    let hooks_dir = hooks_dir()?;
    let payload = serde_json::to_string(hook_input)
        .map_err(|e| format!("encode hook input: {e}"))?;

    for hook in chain {
        let path = hooks_dir.join(&hook);
        if !path.is_file() {
            return Err(format!(
                "[policy-chain] hook missing: {} (declared in policy-chain.toml [{}])",
                path.display(), tool
            ));
        }

        let mut child_cmd = Command::new(&path);
        child_cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        set_process_group(&mut child_cmd);
        apply_safe_env(&mut child_cmd);

        let mut child = child_cmd
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", path.display()))?;
        let pid_opt = child.id();

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(payload.as_bytes()).await
                .map_err(|e| format!("write stdin to {}: {e}", path.display()))?;
            stdin.shutdown().await
                .map_err(|e| format!("close stdin to {}: {e}", path.display()))?;
        }

        let fut = child.wait_with_output();
        let out = match tokio::time::timeout(Duration::from_secs(SAFE_TOOL_TIMEOUT_SECS), fut).await {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => return Err(format!("wait {}: {e}", path.display())),
            Err(_) => {
                if let Some(pid) = pid_opt {
                    killpg_best_effort(pid);
                }
                return Err(format!("hook {hook} timeout"));
            }
        };

        let code = out.status.code().unwrap_or(-1);
        if code == 0 {
            continue;
        }
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!(
            "[blocked by {hook} exit={code}]\n{stderr}"
        ));
    }
    Ok(())
}

/// v0.44 fix #4: async + tokio::fs.
async fn load_chain(tool: &str) -> Result<Vec<String>, String> {
    let path = chain_path()?;
    let exists = fs::try_exists(&path).await.unwrap_or(false);
    if !exists {
        if env_truthy("KEI_POLICY_CHAIN_OPTIONAL") {
            return Ok(vec![]);
        }
        return Err(format!(
            "[policy-chain] config missing: {} (set KEI_POLICY_CHAIN_OPTIONAL=1 to allow pass-through, e.g. for tests)",
            path.display()
        ));
    }
    let raw = fs::read_to_string(&path).await
        .map_err(|e| format!("read policy-chain.toml: {e}"))?;
    let parsed: PolicyChain = toml::from_str(&raw)
        .map_err(|e| format!("parse policy-chain.toml: {e}"))?;
    let chain = match tool {
        "bash"  => parsed.bash.chain,
        "edit"  => parsed.edit.chain,
        "write" => parsed.write.chain,
        _ => return Err(format!("unknown tool kind: {tool}")),
    };
    Ok(chain)
}

fn chain_path() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("KEI_POLICY_CHAIN") {
        return Ok(PathBuf::from(p));
    }
    let dir = hooks_dir()?;
    Ok(dir.join("_lib").join("policy-chain.toml"))
}

fn hooks_dir() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("KEI_HOOKS_DIR") {
        return Ok(PathBuf::from(p));
    }
    let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
    Ok(PathBuf::from(home).join(".claude").join("hooks"))
}

fn env_truthy(name: &str) -> bool {
    matches!(std::env::var(name).as_deref(), Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes"))
}
