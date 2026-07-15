//! Client for OUR kei-cortex agent runtime — Path A (GLM via `/v1/runs`, NO
//! Claude binary). Launches a run and streams its tool/token events into the
//! cockpit's agents sidebar. Default base is the GLM-only runtime on :9800.

use anyhow::{Context, Result};
use futures::StreamExt;
use tokio::sync::mpsc::UnboundedSender;

/// One event from a live agent run, tagged with the run id it belongs to.
#[derive(Debug, Clone)]
pub enum RunEvent {
    Started { id: String, label: String, role: String, task: String },
    Tool { id: String, name: String, phase: String, resource: Option<String>, added: Option<u32>, removed: Option<u32> },
    Delta { id: String, text: String },
    Done { id: String },
    /// The run's REAL provider token usage from the terminal `run.completed`
    /// frame: `input` = prompt tokens (current context, cloud-exact), `output`
    /// = tokens generated. Feeds the real token meters (see `apply_run_event`).
    Usage { id: String, input: u32, output: u32 },
    Error { id: String, msg: String },
    /// A mic transcript came back from the cortex STT endpoint (voice input) —
    /// dropped into the chat input for the user to review + send.
    Voice(String),
    /// A fresh snapshot of every live run/sub-agent from
    /// `GET /api/v1/cortex/activity/stream`. The cockpit filters it to the
    /// sub-agents whose `parent_id` is one of THIS TUI's oracle runs and
    /// renders each as a sidebar card with its live `current_step` — the real
    /// nested-agent view, replacing the tool-use-synthesized stopgap.
    Activity(Vec<RunView>),
}

/// One run/sub-agent row from the cortex activity snapshot. Mirrors
/// `kei-cortex … run_registry::RunView` — only the fields the sidebar needs.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RunView {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub current_step: String,
}

/// Where + how to reach our runtime. Provider is `glm-zai` — our loop, GLM model.
#[derive(Clone, Debug)]
pub struct RunConfig {
    pub base: String,
    pub token: String,
    pub provider: String,
    pub model: String,
    /// Reasoning effort passed to the run (`low`|`medium`|`high`).
    pub effort: String,
}

impl RunConfig {
    /// Base from `KEI_TUI_BASE` (default our GLM-only runtime :9800); bearer from
    /// `~/.keisei/cortex.token`.
    pub fn from_env() -> Self {
        let base = std::env::var("KEI_TUI_BASE").unwrap_or_else(|_| "http://127.0.0.1:9800".into());
        let token = std::env::var("HOME")
            .ok()
            .and_then(|h| std::fs::read_to_string(format!("{h}/.keisei/cortex.token")).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        // Default provider is overridable by env (KEISEIKODE_PROVIDER /
        // KEISEIKODE_MODEL). It is `glm-zai` — Path A, OUR agent_runner driving
        // the model with OUR tools. The `claude` provider shells `claude
        // --print`, whose invoker DISCARDS tool_defs and the delta sink: picking
        // it silently costs the cockpit its tools, its token stream and its
        // agent sidebar. Only fall back to it by hand (`/model claude`).
        let provider = std::env::var("KEISEIKODE_PROVIDER").unwrap_or_else(|_| "glm-zai".into());
        // glm-4.7 does NOT answer on the z.ai anthropic endpoint (empty body);
        // 4.6 and 5.2 do. Never default to a model that isn't smoke-proven.
        let model = std::env::var("KEISEIKODE_MODEL").unwrap_or_else(|_| {
            if provider == "glm-zai" { "glm-4.6".into() } else { "sonnet".into() }
        });
        Self {
            base,
            token,
            provider,
            model,
            effort: "medium".into(),
        }
    }
}

/// POST `/v1/runs` > the new run id.
pub async fn start_run(cfg: &RunConfig, prompt: &str) -> Result<String> {
    start_run_with_image(cfg, prompt, None).await
}

/// Like `start_run` but optionally attaches an image (base64, mime) to the user
/// message — the cockpit sets it when an image file is dropped into the chat, so
/// a vision model (glm-4.6v) sees it.
pub async fn start_run_with_image(
    cfg: &RunConfig,
    prompt: &str,
    image: Option<(String, String)>,
) -> Result<String> {
    let url = format!(
        "{}/v1/runs?provider={}&model={}&effort={}",
        cfg.base, cfg.provider, cfg.model, cfg.effort
    );
    let mut msg = serde_json::json!({ "role": "user", "content": prompt });
    if let Some((b64, mime)) = image {
        msg["image_b64"] = serde_json::Value::String(b64);
        msg["image_mime"] = serde_json::Value::String(mime);
    }
    let body = serde_json::json!({
        "model": cfg.model,
        "messages": [msg],
    });
    let v: serde_json::Value = reqwest::Client::new()
        .post(&url)
        .bearer_auth(&cfg.token)
        .json(&body)
        .send()
        .await
        .context("POST /v1/runs")?
        .json()
        .await
        .context("parse run object")?;
    v.get("id")
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .context("run object had no id")
}

/// POST `/v1/runs/{id}/input` — inject a mid-run user message (steering) so the
/// user can talk to a live agent from the chat pane and it actually reaches the
/// loop (t24). Fire-and-forget from the caller's perspective.
pub async fn send_input(cfg: &RunConfig, run_id: &str, text: &str) -> Result<()> {
    let url = format!("{}/v1/runs/{}/input", cfg.base, run_id);
    reqwest::Client::new()
        .post(&url)
        .bearer_auth(&cfg.token)
        .json(&serde_json::json!({ "content": text }))
        .send()
        .await
        .context("POST /v1/runs/{id}/input")?
        .error_for_status()
        .context("run rejected the steering input")?;
    Ok(())
}

/// GET `/v1/runs/{id}/events` (SSE) > `RunEvent`s until the stream ends.
pub async fn stream_run(cfg: &RunConfig, id: &str, tx: UnboundedSender<RunEvent>) -> Result<()> {
    let url = format!("{}/v1/runs/{}/events", cfg.base, id);
    let resp = reqwest::Client::new()
        .get(&url)
        .bearer_auth(&cfg.token)
        .send()
        .await
        .context("GET /v1/runs/{id}/events")?;
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("sse chunk")?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find("\n\n") {
            let frame: String = buf.drain(..pos + 2).collect();
            for line in frame.lines() {
                let Some(data) = line.trim().strip_prefix("data:") else { continue };
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                let Ok(e) = serde_json::from_str::<serde_json::Value>(data) else { continue };
                if let Some(tool) = e.get("tool").and_then(|x| x.as_str()) {
                    let phase = e.get("phase").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let resource = e.get("resource").and_then(|x| x.as_str()).map(|s| s.to_string());
                    let added = e.get("added").and_then(|x| x.as_u64()).map(|n| n as u32);
                    let removed = e.get("removed").and_then(|x| x.as_u64()).map(|n| n as u32);
                    let _ = tx.send(RunEvent::Tool { id: id.into(), name: tool.into(), phase, resource, added, removed });
                } else if let Some(d) = e.get("delta").and_then(|x| x.as_str()) {
                    let _ = tx.send(RunEvent::Delta { id: id.into(), text: d.into() });
                } else if let Some(u) = e.get("usage") {
                    // Terminal `run.completed` frame — real provider token usage.
                    let input = u.get("prompt_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                    let output = u.get("completion_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                    let _ = tx.send(RunEvent::Usage { id: id.into(), input, output });
                }
            }
        }
    }
    let _ = tx.send(RunEvent::Done { id: id.into() });
    Ok(())
}

/// GET `/api/v1/cortex/activity/stream` (SSE, ~1s tick) — the whole live
/// run/sub-agent snapshot. Each `event: activity` frame carries
/// `{"runs":[RunView…],"terminals":[…]}`; we forward the `runs` as one
/// `RunEvent::Activity`. Runs until the connection drops; the caller keeps a
/// single long-lived subscription for the cockpit's lifetime.
pub async fn stream_activity(cfg: &RunConfig, tx: UnboundedSender<RunEvent>) -> Result<()> {
    let url = format!("{}/api/v1/cortex/activity/stream", cfg.base);
    let resp = reqwest::Client::new()
        .get(&url)
        .bearer_auth(&cfg.token)
        .send()
        .await
        .context("GET /api/v1/cortex/activity/stream")?;
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("activity sse chunk")?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find("\n\n") {
            let frame: String = buf.drain(..pos + 2).collect();
            for line in frame.lines() {
                let Some(data) = line.trim().strip_prefix("data:") else { continue };
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };
                let Some(runs) = v.get("runs") else { continue };
                let views: Vec<RunView> = serde_json::from_value(runs.clone()).unwrap_or_default();
                let _ = tx.send(RunEvent::Activity(views));
            }
        }
    }
    Ok(())
}

/// Keep the activity subscription alive for the whole session: if the stream
/// ends (daemon restart, network blip), wait briefly and reconnect. Spawned
/// once at cockpit startup.
pub fn spawn_activity_stream(cfg: RunConfig, tx: UnboundedSender<RunEvent>) {
    tokio::spawn(async move {
        loop {
            let _ = stream_activity(&cfg, tx.clone()).await;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });
}

/// Render the whole chat transcript into ONE prompt. kei-cortex flattens a
/// run's `messages` into a single user prompt, so replaying both sides as text
/// is what actually gives the agent MEMORY across turns (not just the last line).
fn transcript_prompt(msgs: &[crate::chat::Msg]) -> String {
    let mut s = String::from(
        "You are continuing an ongoing conversation. Use the full history below \
for context, then respond as the assistant to the LAST user message.\n\n",
    );
    for m in msgs {
        let t = m.text.trim();
        if t.is_empty() {
            continue;
        }
        let who = match m.role {
            crate::chat::Role::User => "User",
            crate::chat::Role::Agent => "Assistant",
            // Tool action lines are UI-only — never replayed to the model.
            crate::chat::Role::Tool => continue,
        };
        s.push_str(who);
        s.push_str(": ");
        s.push_str(t);
        s.push_str("\n\n");
    }
    s.push_str("Assistant:");
    s
}

/// Fire a run carrying the FULL chat history (memory). The agent sees every
/// prior user + assistant turn, not just the newest line.
pub fn spawn_run_messages(
    cfg: RunConfig,
    msgs: Vec<crate::chat::Msg>,
    label: String,
    role: String,
    task: String,
    tx: UnboundedSender<RunEvent>,
) {
    spawn_run_messages_image(cfg, msgs, label, role, task, tx, None)
}

/// Like `spawn_run_messages` but attaches an image to the (last) user message.
#[allow(clippy::too_many_arguments)]
pub fn spawn_run_messages_image(
    cfg: RunConfig,
    msgs: Vec<crate::chat::Msg>,
    label: String,
    role: String,
    task: String,
    tx: UnboundedSender<RunEvent>,
    image: Option<(String, String)>,
) {
    let prompt = transcript_prompt(&msgs);
    spawn_run_image(cfg, prompt, label, role, task, tx, image);
}

/// Fire a run in the background: Started > tool/token events > Done/Error.
/// `role`/`task` are surfaced in the agent card + detail view.
pub fn spawn_run(
    cfg: RunConfig,
    prompt: String,
    label: String,
    role: String,
    task: String,
    tx: UnboundedSender<RunEvent>,
) {
    spawn_run_image(cfg, prompt, label, role, task, tx, None)
}

/// `spawn_run` with an optional attached image (vision).
#[allow(clippy::too_many_arguments)]
pub fn spawn_run_image(
    cfg: RunConfig,
    prompt: String,
    label: String,
    role: String,
    task: String,
    tx: UnboundedSender<RunEvent>,
    image: Option<(String, String)>,
) {
    tokio::spawn(async move {
        match start_run_with_image(&cfg, &prompt, image).await {
            Ok(id) => {
                let _ = tx.send(RunEvent::Started { id: id.clone(), label, role, task });
                if let Err(e) = stream_run(&cfg, &id, tx.clone()).await {
                    let _ = tx.send(RunEvent::Error { id, msg: e.to_string() });
                }
            }
            Err(e) => {
                let _ = tx.send(RunEvent::Error { id: "-".into(), msg: e.to_string() });
            }
        }
    });
}
