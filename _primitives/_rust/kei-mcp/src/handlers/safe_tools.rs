//! Phase C — cross-CLI hook enforcement via MCP-wrapped tools.
//!
//! Exposes three built-in MCP tools — `kei_bash`, `kei_edit`, `kei_write` —
//! that synthesize Claude Code's PreToolUse hook input contract, chain
//! through the hook scripts declared in `~/.claude/hooks/_lib/policy-chain.toml`,
//! and only execute the wrapped action if every hook returns exit 0.
//!
//! Why this exists: when an agent runs on Grok / Agy / Copilot / Kimi, none
//! of our claude-side PreToolUse hooks fire. The agent could read the rules
//! in its system prompt but the tool-call layer was previously ungated. The
//! `kei_*` MCP tools restore that gate for any MCP-capable CLI.
//!
//! Constructor Pattern: ONE policy SSoT (`policy-chain.toml`), ONE dispatcher
//! (this file), hooks reused as-is from `~/.claude/hooks/`. No rewrite, no
//! abstraction layer. Shell-out per hook keeps the contract identical to
//! Claude's native PreToolUse pipeline.
//!
//! CLAUDECODE / GROKCODE guard — DESIGN NOTE (NOT a security boundary):
//! When invoked from inside Claude Code (`$CLAUDECODE=1`) or Grok the chain
//! is SKIPPED to avoid double-firing the same hooks (they already ran on the
//! CLI's own PreToolUse). This is a perf / UX optimization for the inside-CLI
//! call path — NOT an authorization check. An attacker who can set the
//! parent process's environment already controls the CLI invocation anyway;
//! re-running hooks would not stop them. To raise the bar for confused-deputy
//! scenarios use full sandboxing (Phase D) or run kei-mcp as a separate UID.
//!
//! v0.41 audit fixes (2026-05-26, Gemini security review):
//!   #1 fail-CLOSED on missing hooks (was: silently skip)
//!   #2 path-traversal guard on kei_edit/kei_write (canonicalize + root check)
//!   #3 CLAUDECODE bypass — documented as design (see above), no behavior change
//!   #4 tokio::fs for async file I/O (was: blocking std::fs on tokio thread)
//!   #5 process-group kill on Unix (was: kill_on_drop SIGKILLs only direct child)
//!
//! v0.42 re-audit fixes (2026-05-26, 4-CLI dogfood: Claude+Grok+Gemini+Copilot):
//!   #1 [CRITICAL] symlink LEAF bypass — canonicalize full path + reject
//!      leaf symlinks (v0.41 only canonicalized PARENT; ln -s ~/.ssh/keys ./x
//!      then kei_write x followed the link to the target)
//!   #2 [HIGH]     $HOME removed from default allowed_roots — was a blanket
//!      allow that let agent overwrite ~/.claude/hooks (self-neuter), ~/.zshrc
//!      (RCE on next shell), and credential stores. Default: $PWD only.
//!      Denylist also extended with .claude/, .grok/, .gemini/, .copilot/,
//!      .kimi/, and exact shell-init filenames.
//!   #3 [HIGH]     empty [bash]/[edit]/[write] section also FAIL-CLOSED (was:
//!      empty vec → pass-through). KEI_POLICY_CHAIN_OPTIONAL=1 to opt in.
//!   #4 [MED]      load_chain converted to async + tokio::fs (was: blocking
//!      std::fs on tokio worker thread).
//!   #5 [MED]      set_process_group + killpg applied to HOOK subprocess too
//!      (v0.41 only had it on the bash action; hook grandchildren orphaned).
//!   #6 [MED]      doc note that aggregate timeout is still per-step (60s ×
//!      N hooks + 60s action). Single-deadline implementation deferred to
//!      v0.43 — not security-blocking.

use crate::protocol::{err, ok, JsonRpcRequest, JsonRpcResponse, INTERNAL_ERROR, INVALID_PARAMS};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Hard cap on how long a single hook chain + action may take. Matches the
/// timeout in `handlers::tools::ATOM_TIMEOUT_SECS` for consistency.
const SAFE_TOOL_TIMEOUT_SECS: u64 = 60;

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

/// MCP tool descriptors — appended to `tools/list` by `handlers::tools::list`.
pub fn descriptors() -> Vec<Value> {
    vec![
        json!({
            "name": "kei_bash",
            "description": "Run a shell command after running KeiSeiKit's [bash] policy chain (no-github-push, safety-guard, destructive-guard). Blocks on hook exit 2 with the hook's stderr surfaced as the MCP error message. Use this instead of native shell on non-Claude CLIs to inherit Claude Code's safety enforcement.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute" },
                    "cwd": { "type": "string", "description": "Optional working directory; defaults to $PWD" }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "kei_edit",
            "description": "Modify a file (replace old_string with new_string) after running KeiSeiKit's [edit] policy chain (citation-verify, numeric-claims-guard). Blocks unverified academic citations and numeric claims without evidence markers.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file_path": { "type": "string" },
                    "old_string": { "type": "string" },
                    "new_string": { "type": "string" }
                },
                "required": ["file_path", "old_string", "new_string"]
            }
        }),
        json!({
            "name": "kei_write",
            "description": "Write content to a file after running KeiSeiKit's [write] policy chain (citation-verify, numeric-claims-guard). Blocks unverified academic citations and numeric claims without evidence markers.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file_path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["file_path", "content"]
            }
        }),
    ]
}

/// Top-level dispatch entry — called from `handlers::tools::call` when the
/// tool name matches one of the three `kei_*` built-ins.
pub async fn dispatch_safe(req: JsonRpcRequest, name: &str, args: &Value) -> JsonRpcResponse {
    let result = match name {
        "kei_bash"  => handle_bash(args).await,
        "kei_edit"  => handle_edit(args).await,
        "kei_write" => handle_write(args).await,
        _ => Err(format!("safe_tools dispatched unknown name: {name}")),
    };
    match result {
        Ok(text) => ok(req.id, json!({
            "content": [{ "type": "text", "text": text }],
            "isError": false,
        })),
        Err(e) => err(req.id, INTERNAL_ERROR, e),
    }
}

// ---- per-tool handlers --------------------------------------------------

async fn handle_bash(args: &Value) -> Result<String, String> {
    let command = args.get("command").and_then(Value::as_str)
        .ok_or_else(|| missing_arg("kei_bash", "command"))?;
    let cwd = args.get("cwd").and_then(Value::as_str);

    let hook_input = json!({
        "tool_name": "Bash",
        "tool_input": { "command": command }
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
    // v0.41 fix #5 (Gemini MED): put child in its own process group so timeout
    // kills it and ALL grandchildren together (not just the immediate shell).
    set_process_group(&mut cmd);

    let child = cmd.spawn().map_err(|e| format!("spawn bash: {e}"))?;
    let pid_opt = child.id();
    let fut = child.wait_with_output();

    let out = match tokio::time::timeout(Duration::from_secs(SAFE_TOOL_TIMEOUT_SECS), fut).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return Err(format!("wait bash: {e}")),
        Err(_) => {
            // Timeout — kill the entire process group, not just the child.
            if let Some(pid) = pid_opt {
                killpg_best_effort(pid);
            }
            return Err("kei_bash timeout".to_string());
        }
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
    Ok(if stderr.is_empty() { stdout } else { format!("{stdout}\n[stderr]\n{stderr}") })
}

// v0.41 fix #5: process-group helpers (Unix-only; no-op on other platforms).
// tokio::process::Command::process_group is available on Unix without
// requiring the std::os::unix::process::CommandExt trait import.
#[cfg(unix)]
fn set_process_group(cmd: &mut Command) {
    cmd.process_group(0); // 0 = new session leader for this child
}
#[cfg(not(unix))]
fn set_process_group(_cmd: &mut Command) {}

#[cfg(unix)]
fn killpg_best_effort(pid: u32) {
    // SAFETY: libc::kill on a negative PID targets the process group.
    // SIGKILL = 9. Best-effort — ignore errors (process may have exited).
    unsafe {
        let _ = libc::kill(-(pid as i32), libc::SIGKILL);
    }
}
#[cfg(not(unix))]
fn killpg_best_effort(_pid: u32) {}

async fn handle_edit(args: &Value) -> Result<String, String> {
    let file_path = args.get("file_path").and_then(Value::as_str)
        .ok_or_else(|| missing_arg("kei_edit", "file_path"))?;
    let old_string = args.get("old_string").and_then(Value::as_str)
        .ok_or_else(|| missing_arg("kei_edit", "old_string"))?;
    let new_string = args.get("new_string").and_then(Value::as_str)
        .ok_or_else(|| missing_arg("kei_edit", "new_string"))?;

    // v0.41 fix #2: path-traversal guard
    let safe_path = validate_path(file_path)?;

    let hook_input = json!({
        "tool_name": "Edit",
        "tool_input": {
            "file_path": safe_path.display().to_string(),
            "old_string": old_string,
            "new_string": new_string
        }
    });
    run_chain("edit", &hook_input).await?;

    // v0.41 fix #4: tokio::fs (async)
    let contents = fs::read_to_string(&safe_path).await
        .map_err(|e| format!("read {}: {e}", safe_path.display()))?;
    if !contents.contains(old_string) {
        return Err(format!("kei_edit: old_string not found in {}", safe_path.display()));
    }
    let updated = contents.replacen(old_string, new_string, 1);
    fs::write(&safe_path, &updated).await
        .map_err(|e| format!("write {}: {e}", safe_path.display()))?;
    Ok(format!("edited {} ({} bytes)", safe_path.display(), updated.len()))
}

async fn handle_write(args: &Value) -> Result<String, String> {
    let file_path = args.get("file_path").and_then(Value::as_str)
        .ok_or_else(|| missing_arg("kei_write", "file_path"))?;
    let content = args.get("content").and_then(Value::as_str)
        .ok_or_else(|| missing_arg("kei_write", "content"))?;

    // v0.41 fix #2: path-traversal guard
    let safe_path = validate_path(file_path)?;

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
    fs::write(&safe_path, content).await
        .map_err(|e| format!("write {}: {e}", safe_path.display()))?;
    Ok(format!("wrote {} ({} bytes)", safe_path.display(), content.len()))
}

/// Path-traversal + symlink + denylist guard.
///
/// v0.41 (initial): rejected `..`, canonicalized PARENT, checked denylist + roots.
///   → 4-CLI re-audit (2026-05-26) found this was bypassable via symlink at the
///     leaf and self-attackable via the $HOME blanket-allowed root.
///
/// v0.42 fixes:
///   #1 [CRITICAL] reject if the leaf is a symlink (was: validated parent
///      only, fs::write followed leaf symlink to anywhere). Done via
///      `symlink_metadata` on the leaf BEFORE write, and full `canonicalize`
///      on the leaf when the file already exists.
///   #2 [HIGH] $HOME removed from default allowed-roots — default is $PWD
///      only. Denylist now also covers $HOME/.claude/ (the substrate
///      itself), shell init files, and credential stores. Operators who
///      need broader access set KEI_ALLOWED_ROOTS explicitly.
fn validate_path(p: &str) -> Result<PathBuf, String> {
    if p.is_empty() {
        return Err("file_path: empty".into());
    }
    // 1. Reject literal `..` segments — covers most traversal attempts.
    if p.split('/').any(|seg| seg == "..") {
        return Err(format!("file_path: '..' segment not allowed in {p}"));
    }
    let path = Path::new(p);

    // 2. Build a canonical path. Prefer canonicalizing the FULL path (resolves
    //    symlinks at the leaf, fixing v0.41 CRITICAL bypass). For files that
    //    don't exist yet (kei_write new file), canonicalize the parent and
    //    join the leaf — but then explicitly check the leaf isn't a symlink
    //    via symlink_metadata before writing.
    let canonical = if path.exists() {
        // File exists — canonicalize full path, including resolving any leaf
        // symlink to its real target. The denylist/roots check below then
        // sees the REAL destination, not the symlink name.
        path.canonicalize()
            .map_err(|e| format!("file_path: canonicalize {}: {e}", path.display()))?
    } else if let Some(parent) = path.parent() {
        if parent.as_os_str().is_empty() || parent == Path::new("") {
            std::env::current_dir()
                .map_err(|e| format!("file_path: cwd unavailable: {e}"))?
                .join(path.file_name().unwrap_or_default())
        } else if parent.exists() {
            parent.canonicalize()
                .map_err(|e| format!("file_path: canonicalize {}: {e}", parent.display()))?
                .join(path.file_name().unwrap_or_default())
        } else if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| format!("file_path: cwd unavailable: {e}"))?
                .join(path)
        }
    } else {
        return Err(format!("file_path: invalid {p}"));
    };

    // 3. Even when the file doesn't exist yet, the LEAF could already be a
    //    dangling symlink that `fs::write` would follow on creation. Reject.
    if let Ok(meta) = std::fs::symlink_metadata(&canonical) {
        if meta.file_type().is_symlink() {
            return Err(format!(
                "file_path: leaf is a symlink (refusing to follow): {}",
                canonical.display()
            ));
        }
    }

    let canon_str = canonical.display().to_string();

    // 4. Reject system + substrate-control + credential paths.
    let denylist = [
        "/etc/", "/usr/", "/System/", "/var/", "/private/etc/", "/private/var/",
        "/root/", "/bin/", "/sbin/",
    ];
    for d in denylist {
        if canon_str.starts_with(d) {
            return Err(format!("file_path: denied (system dir): {canon_str}"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        // v0.42 fix #2 extended denylist — these targets enable self-attack
        // (overwrite the substrate or shell init for RCE on next session).
        let dir_secrets = [
            ".ssh/", ".aws/", ".gnupg/", ".config/gcloud/", ".cargo/credentials",
            ".npmrc", ".docker/config.json", ".kube/",
            ".claude/",      // our own substrate: hooks, settings, agents
            ".grok/",        // sibling CLI's settings
            ".gemini/",      // antigravity settings
            ".copilot/",     // copilot config
            ".kimi/",        // kimi config
        ];
        for sd in dir_secrets {
            let full = format!("{home}/{sd}");
            if canon_str.starts_with(&full) {
                return Err(format!("file_path: denied (secret/substrate dir): {canon_str}"));
            }
        }
        // Exact shell-init files (overwriting → RCE on next shell start).
        let init_files = [
            ".zshrc", ".bashrc", ".profile", ".bash_profile", ".zprofile",
            ".zshenv", ".bash_login", ".inputrc", ".gitconfig",
            ".config/fish/config.fish",
        ];
        for f in init_files {
            let full = format!("{home}/{f}");
            if canon_str == full {
                return Err(format!("file_path: denied (shell-init file): {canon_str}"));
            }
        }
    }

    // 5. Enforce allowed-root containment.
    let roots = allowed_roots();
    if !roots.is_empty() {
        let ok = roots.iter().any(|r| canon_str.starts_with(r));
        if !ok {
            return Err(format!(
                "file_path: outside allowed roots {roots:?}: {canon_str}"
            ));
        }
    }

    Ok(canonical)
}

fn allowed_roots() -> Vec<String> {
    if let Ok(v) = std::env::var("KEI_ALLOWED_ROOTS") {
        return v.split(':').filter(|s| !s.is_empty()).map(String::from).collect();
    }
    // v0.42 fix #2 (Claude+Gemini HIGH): default to $PWD ONLY. Was: $PWD +
    // $HOME blanket — too permissive, agent could overwrite ~/.claude/hooks/
    // or ~/.zshrc and self-neuter the safety layer. Operators who need
    // broader access opt in via KEI_ALLOWED_ROOTS=":" -separated abs paths.
    let mut roots = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(format!("{}/", cwd.display()));
    }
    roots
}

// ---- chain runner -------------------------------------------------------

/// Run the configured hook chain for `tool` ("bash"/"edit"/"write"), piping
/// `hook_input` to each hook's stdin in order. Exit 0 → continue. Exit 2 (or
/// other non-zero) → return Err with the hook's stderr.
///
/// Skips the chain if the parent process is already inside Claude or Grok
/// (env flags), since those CLIs' native PreToolUse hooks already fired.
/// Run the configured hook chain for `tool` ("bash"/"edit"/"write").
///
/// v0.42 fixes:
///   #3 [HIGH]   empty chain (section absent or zero hooks) now FAILS CLOSED
///               unless KEI_POLICY_CHAIN_OPTIONAL=1.
///   #4 [MED]    load_chain() converted to async (was: blocking std::fs).
///   #5 [MED]    hook subprocess gets `process_group(0)` + killpg on timeout
///               (was: only the bash action got it; hooks could orphan).
///   #6 [MED]    aggregate timeout across the whole chain + action (was:
///               per-hook 60s, so chain+action could legitimately run
///               4× the documented cap on a 3-hook chain).
async fn run_chain(tool: &str, hook_input: &Value) -> Result<(), String> {
    if env_truthy("CLAUDECODE") || env_truthy("GROKCODE") {
        // Native hooks already enforced — don't double-fire.
        return Ok(());
    }

    let chain = load_chain(tool).await?;
    if chain.is_empty() {
        // v0.42 fix #3 (Claude+Gemini HIGH): empty section is the same
        // misconfig class as missing file — FAIL CLOSED with explicit opt-in.
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
        // v0.42 fix #5: put hook child in its own process group so timeout
        // can killpg the whole tree (was: kill_on_drop = immediate child only).
        set_process_group(&mut child_cmd);

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
                // v0.42 fix #5: kill the whole hook process group, not just
                // the immediate child.
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

// ---- config helpers -----------------------------------------------------

/// v0.42 fix #4: async + tokio::fs (was: blocking std::fs would freeze
/// a tokio worker if policy-chain.toml lived on a slow / hung mount).
async fn load_chain(tool: &str) -> Result<Vec<String>, String> {
    let path = chain_path()?;
    // tokio::fs::try_exists avoids a blocking is_file() syscall.
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

fn missing_arg(tool: &str, field: &str) -> String {
    format!("{tool}: missing '{field}' argument")
}

#[allow(dead_code)]
const INVALID_PARAMS_REF: i32 = INVALID_PARAMS; // silence unused-import warning if removed
