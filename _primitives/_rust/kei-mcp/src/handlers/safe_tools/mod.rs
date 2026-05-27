//! Phase C — cross-CLI hook enforcement via MCP-wrapped tools.
//!
//! v0.46: decomposed from single safe_tools.rs (738 LOC, god-object per
//! architect audit) into 5 focused modules:
//!
//!   mod.rs          — descriptor list + tools/call dispatch (this file)
//!   chain_runner.rs — load_chain + run_chain (policy enforcement engine)
//!   path_guard.rs   — validate_path + canonicalize-with-walk-up + allowed_roots
//!   exec.rs         — handle_bash/edit/write + O_NOFOLLOW open + write paths
//!   env_guard.rs    — apply_safe_env + set_process_group + KillPgGuard (RAII)
//!
//! Exposes three built-in MCP tools — `kei_bash`, `kei_edit`, `kei_write` —
//! that synthesize Claude Code's PreToolUse hook input contract and chain
//! through the hook scripts in `~/.claude/hooks/_lib/policy-chain.toml`.
//!
//! v0.46 architectural fix #1 (Claude critic CRITICAL): REMOVED env-based
//! chain-skip (was `CLAUDECODE=1` / `GROKCODE=1` → skip). Rationale: those
//! envs were set assuming "if we're inside Claude/Grok, native PreToolUse
//! already fires — skip our chain to avoid double-firing". But native
//! PreToolUse matchers fire on tool_name = "Bash"|"Edit"|"Write" — these
//! MCP tools are named `kei_bash`/`kei_edit`/`kei_write` (or with mcp__
//! prefix). Native hooks therefore NEVER fire on these calls, and the
//! env-skip created a real auth-bypass hole on Grok. Chain now ALWAYS
//! runs; the perf concern was fictional.

use crate::protocol::{err, ok, JsonRpcRequest, JsonRpcResponse, INTERNAL_ERROR};
use serde_json::{json, Value};

mod chain_runner;
mod env_guard;
mod exec;
mod path_guard;

/// Per-step timeout (each hook AND the action each get up to this long).
/// For an N-hook chain the total wall-clock cap is approximately
/// `(N+1) * SAFE_TOOL_TIMEOUT_SECS`. v0.44 doc-honesty: prior versions
/// claimed this was an "aggregate" cap which was always wrong.
pub(crate) const SAFE_TOOL_TIMEOUT_SECS: u64 = 60;

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

/// Dispatch entry — called from `handlers::tools::call` when the tool name
/// matches one of the three `kei_*` built-ins.
pub async fn dispatch_safe(req: JsonRpcRequest, name: &str, args: &Value) -> JsonRpcResponse {
    let result = match name {
        "kei_bash"  => exec::handle_bash(args).await,
        "kei_edit"  => exec::handle_edit(args).await,
        "kei_write" => exec::handle_write(args).await,
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
