//! Cockpit state: the three live panes + focus + an optional open agent detail.
//! Event wiring lives in `runner`; this module is the state those events mutate.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;

use crate::agents::{AgentCard, AgentStatus, AgentsPane};
use crate::chat::{ChatPane, Role};
use crate::editor::EditorPane;
use crate::runs::{RunConfig, RunEvent};
use crate::settings::SettingsPane;
use crate::term::TerminalPane;
use crate::tree::TreePane;
use crate::types::Pane;

/// The model's context window in tokens — the denominator of the bottom-left
/// context meter and the top-right window readout. Every model we currently
/// reach (GLM-4.6/4.7/5.2 on z.ai, Claude Sonnet/Opus) advertises a 200K
/// window, so one constant covers them; revisit when a smaller-window model is
/// wired in.
pub const MAX_CONTEXT_TOKENS: u32 = 200_000;

/// What the center pane shows. Default is `Chat` — the primary interface, like
/// Claude Code / opencode. When a file is open (`editor.is_open()`) the center
/// is split editor-above-chat; otherwise the chat fills it. `Terminal`,
/// `Agent`, `Settings` are secondary modes toggled by keys/clicks; Esc returns
/// to `Chat` (closing the editor first if one is open).
pub enum CenterMode {
    /// The chat (primary). Editor rides ABOVE it when a file is open.
    Chat,
    /// Embedded shell-PTY (toggle).
    Terminal,
    /// Live detail of one agent expanded to the whole center (t12 / t23).
    Agent(String),
    /// Settings panel opened from the left-sidebar bottom tab (t22).
    Settings,
    /// The project plan / passport opened full-screen (`/ps`, `/plan`, Ctrl-P,
    /// or clicking a task in the right sidebar).
    Plan,
    /// The AGENTS DASHBOARD (Claude-Code-style): this session's agents grouped
    /// by status (needs-input / working / completed) + what each is doing.
    /// Opened by /a or the bottom "agents" indicator.
    Agents,
    /// An agent's DNA (manifest) view + editor, full-screen. Reached by drilling
    /// `/agentslib` → an agent in the command palette.
    Dna,
    /// An inline image (screenshot) shown full-screen. `usize` = the id into the
    /// `ImagePane` cache. Opened by `/screenshot`; Esc back to chat.
    Image(usize),
}

/// What the LEFT sidebar shows: the file tree, or the live "structure" of the
/// running agents/oracle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftView {
    Files,
    Structure,
}

pub struct App {
    pub focus: Pane,
    pub should_quit: bool,
    pub tree: TreePane,
    pub term: TerminalPane,
    pub agents: AgentsPane,
    pub editor: EditorPane,
    pub chat: ChatPane,
    /// The slash-command palette (Claude-Code / Grok style pop-up). Open while
    /// the user is picking a `/command`; drives arrow navigation over the
    /// categorised command list instead of dumping help text into the chat.
    pub palette: crate::palette::CommandPalette,
    /// The agent-DNA (manifest) view + editor shown by CenterMode::Dna.
    pub dna: crate::dna::DnaPane,
    /// Inline-image renderer (screenshots) — CenterMode::Image + chat images.
    pub images: crate::image_pane::ImagePane,
    /// An image attached to the NEXT message (base64, mime) — set by dropping an
    /// image file into the chat; consumed by the next send → glm-4.6v vision.
    pub pending_image: Option<(String, String)>,
    /// Shell-style command recall: how many steps back into the user's own
    /// message history the Up-arrow has walked (0 = not recalling). Reset on any
    /// edit / send.
    pub recall_idx: usize,
    pub settings: SettingsPane,
    /// Project `.mf` passport viewer in the right sidebar's bottom half (t40).
    pub passport: crate::mf_view::PassportPane,
    /// What the center pane shows.
    pub center: CenterMode,
    /// The ORACLE run (the main chat session). Its live TEXT mirrors into the
    /// chat and it is what Enter steers. It is NEVER shown as an agent card —
    /// the agents sidebar shows ONLY sub-agents the oracle launched.
    pub chat_agent: Option<String>,
    /// Every oracle run id THIS TUI has started (I3: "current session" is
    /// client-side). The activity stream is global; a sub-agent belongs to this
    /// cockpit iff its `parent_id` is in here — that's the sidebar filter.
    pub oracle_runs: HashSet<String>,
    /// True while the oracle run is in flight — drives the spinning Frobenius
    /// sphere in the chat and the "steer vs new-run" decision on send.
    pub oracle_busy: bool,
    /// The oracle's currently-running tool (bash/read/edit/…) — drives the live
    /// activity dot on the status bar. Set on a tool-start, cleared when the
    /// model streams text again or the run finishes.
    pub oracle_tool: Option<String>,
    /// When the current oracle run started (for the spinner's elapsed clock).
    pub oracle_started: Instant,
    /// App-start instant — a CONTINUOUS clock (never resets) that drives the
    /// always-spinning header Frobenius sphere.
    pub app_started: Instant,
    /// Tokens spent by the oracle session (it has no card, so counted here).
    pub oracle_tokens: u32,
    /// Tokens of the CURRENT (or last) oracle request only — reset on each send,
    /// counted live from streamed words, then replaced by the provider's real
    /// output count on the run's `Usage`. Shown compactly on the status bar so
    /// the per-request cost is visible.
    pub run_tokens: u32,
    /// Rolling total for the DAY (all runs) — resets at local midnight.
    pub day_tokens: u32,
    /// Local date (YYYY-MM-DD) the `day_tokens` counter belongs to.
    pub day_stamp: String,
    /// Day-total across EVERY session (read from ~/.keisei/token-events.sqlite by
    /// a background tick). `None` until the first read / when the DB is absent.
    pub day_all_tokens: Option<u64>,
    /// Cached REAL BPE token count of the current context (transcript + input),
    /// recomputed by a tick only when the cheap signature below changes — a full
    /// re-encode every frame would be O(chars) × 7fps on a long chat.
    pub context_tok_cache: u32,
    /// Cheap change-signature (Σ chars of msgs + input len) gating the recompute.
    pub context_tok_sig: usize,
    /// The provider's EXACT prompt-token count from the last run's `Usage`
    /// (cloud, or a local openai-compat server that reports usage). Preferred
    /// over the BPE estimate when present + non-zero. `None` until one arrives.
    pub provider_context: Option<u32>,
    /// Where the user's own messages sit in the chat: right (default) or left.
    pub user_right: bool,
    /// Active project (`/cp <project>`) — its oracle is adopted + passport shown.
    pub project: Option<String>,
    /// When a file is open ABOVE the chat, this routes typing to the EDITOR
    /// (edit the code) instead of the chat input. Toggled by F6 / clicking the
    /// editor; Esc / Ctrl-S returns it to the chat.
    pub editor_focus: bool,
    /// When `Some(i)`, focus is DOWN in the mode bar on control `i`
    /// (0 auto · 1 accept-edits · 2 plan · 3 mic · 4 speaker) — <> navigate,
    /// Enter toggles, Up returns to the chat. `None` = normal chat focus.
    pub bar_focus: Option<usize>,
    /// Speaker is "speaking" (green) until this instant — set when a TTS reply
    /// starts, so the icon shows the three states gray/white/green.
    pub speaking_until: Option<Instant>,
    /// The most recent file the oracle edited (from a tool event's resource) —
    /// clicking a "> edit …" line in the chat opens it in the editor.
    pub last_edit_path: Option<PathBuf>,
    /// True while the mouse button is held down on the "mic" label (push-to-
    /// hold recording) — released on mouse-up.
    pub mic_held: bool,
    pub status: String,
    /// True while a left-drag that began on the file tree is in flight (t04).
    pub dragging_from_tree: bool,
    /// Left sidebar collapsed to a thin strip (Ctrl-B / click the ‹ toggle).
    pub tree_collapsed: bool,
    /// Right sidebar (agents + passport) collapsed to a thin strip (Ctrl-L).
    pub right_collapsed: bool,
    /// PIN model: a full-screen window (passport / agents / files) is by default
    /// transient — you open it, look, Esc back to chat. Press P to PIN it into a
    /// sidebar so it stays alongside the chat; P again unpins. The sidebars show
    /// ONLY what is pinned.
    pub pin_passport: bool,
    pub pin_agents: bool,
    pub pin_tree: bool,
    /// Left sidebar view: file tree vs running-agents structure.
    pub left_view: LeftView,
    /// Approval toggle (ONE button, auto↔accept-edits): true = auto (green),
    /// false = accept-edits (yellow).
    pub approval_auto: bool,
    /// Plan mode on/off (separate button). When on, a chat send writes a `.mf`
    /// plan node instead of launching the oracle.
    pub plan_mode: bool,
    /// Current chat session id (session-<id>.json). `/new` mints a fresh one.
    pub session_id: String,
    /// `/model <name>` override for new runs; `None` > the RunConfig default.
    pub model_override: Option<String>,
    /// Reasoning effort for new runs (`/effort low|medium|high`).
    pub effort: String,
    /// When the mode bar was last touched (nav or toggle) — the bar shows for a
    /// couple seconds after, then collapses so the bottom is ONE line.
    pub bar_touched_at: Option<Instant>,
    /// Mic capture in flight (push-to-talk, Ctrl-Space). `Some` ⇒ recording.
    pub recorder: Option<crate::voice::Recorder>,
    /// Speak agent replies aloud via TTS when on (toggle with Ctrl-R).
    pub voice_reply: bool,
    /// Provider + backend shown in the settings panel (t22).
    pub provider: String,
    pub base_url: String,
    launched: u32,
}

impl App {
    /// Build the cockpit rooted at `cwd` (spawns the shell at a provisional size;
    /// the real size is applied on the first render).
    pub fn new(cwd: PathBuf) -> Result<Self> {
        let term = TerminalPane::new(24, 80, &cwd)?;
        let cfg = RunConfig::from_env();
        // Reload the last session's transcript so the chat survives a restart.
        let mut chat = ChatPane::new();
        chat.msgs = crate::session::load_last();
        // The banner rides as the first row of the scrollable history: visible
        // at the top when the chat is empty, scrolls away as it fills.
        chat.banner = Some(format!(
            "KEISEIKODE {} · {} · {} · {}",
            crate::header::VERSION, cfg.model, cfg.provider, cwd.display()
        ));
        let session_id = crate::session::last_id().unwrap_or_else(crate::session::new_id);
        Ok(Self {
            // Chat is the primary center > focus the center slot on start so
            // typing goes straight to the chat input (matches "type to chat").
            focus: Pane::Terminal,
            should_quit: false,
            tree: TreePane::new(cwd),
            term,
            agents: AgentsPane::new(),
            editor: EditorPane::new(),
            chat,
            palette: crate::palette::CommandPalette::default(),
            dna: crate::dna::DnaPane::default(),
            images: crate::image_pane::ImagePane::new(),
            pending_image: None,
            recall_idx: 0,
            settings: SettingsPane::new(),
            passport: crate::mf_view::PassportPane::new(),
            center: CenterMode::Chat,
            chat_agent: None,
            oracle_runs: HashSet::new(),
            oracle_busy: false,
            oracle_tool: None,
            oracle_started: Instant::now(),
            app_started: Instant::now(),
            oracle_tokens: 0,
            run_tokens: 0,
            day_tokens: 0,
            day_stamp: today_local(),
            day_all_tokens: None,
            context_tok_cache: 0,
            context_tok_sig: usize::MAX, // force a first-tick recompute
            provider_context: None,
            // The user's own messages sit on the LEFT by default (settings can
            // flip to the right).
            user_right: false,
            project: None,
            editor_focus: false,
            bar_focus: None,
            speaking_until: None,
            last_edit_path: None,
            mic_held: false,
            status: "type to chat · /help commands · /f files · /a agents · Ctrl-Q quit".into(),
            dragging_from_tree: false,
            // Sidebars are HIDDEN by default (clean central chat, like Claude
            // Code). /f shows the file tree, /a the agents panel.
            tree_collapsed: true,
            right_collapsed: true,
            pin_passport: false,
            pin_agents: false,
            pin_tree: false,
            left_view: LeftView::Files,
            approval_auto: true,
            plan_mode: false,
            session_id,
            model_override: None,
            effort: "medium".into(),
            bar_touched_at: None,
            recorder: None,
            voice_reply: false,
            provider: cfg.provider,
            base_url: cfg.base,
            launched: 0,
        })
    }

    /// Persist the current chat transcript (best-effort) — called after every
    /// chat turn so history survives a restart.
    pub fn save_session(&self) {
        crate::session::save(&self.session_id, &self.chat.msgs);
    }

    /// The user's own past messages, NEWEST first, de-duplicated — the source
    /// for shell-style Up-arrow command recall + the history window.
    pub fn user_history(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for m in self.chat.msgs.iter().rev() {
            if m.role == Role::User {
                let t = m.text.trim();
                if !t.is_empty() && seen.insert(t.to_string()) {
                    out.push(t.to_string());
                }
            }
        }
        out
    }

    /// The run config for a new agent, honouring the `/model` override.
    pub fn run_config(&self) -> crate::runs::RunConfig {
        let mut cfg = crate::runs::RunConfig::from_env();
        cfg.provider = self.provider.clone();
        if let Some(m) = &self.model_override {
            cfg.model = m.clone();
        }
        cfg.effort = self.effort.clone();
        cfg
    }

    /// True when the mode bar (2nd bottom line) should show: focus is in it, or
    /// it was toggled in the last ~2.5s (a fleeting hint). Otherwise the bottom
    /// is a single status line.
    pub fn bar_visible(&self) -> bool {
        self.bar_focus.is_some()
            || self
                .bar_touched_at
                .map(|t| t.elapsed().as_millis() < 2500)
                .unwrap_or(false)
    }

    /// Mark the mode bar recently touched (shows it briefly).
    pub fn touch_bar(&mut self) {
        self.bar_touched_at = Some(Instant::now());
    }

    /// Start a brand-new chat session (`/new`): archive the current one, mint a
    /// fresh id, clear the transcript.
    pub fn new_session(&mut self) {
        self.save_session();
        self.session_id = crate::session::new_id();
        self.chat.msgs.clear();
        self.chat_agent = None;
        self.oracle_busy = false;
        self.provider_context = None; // exact context belongs to the old session
        self.save_session();
    }

    /// Load a saved session by id into the chat (`/sessions <id>`).
    pub fn load_session(&mut self, id: &str) {
        self.save_session();
        self.session_id = id.to_string();
        self.chat.msgs = crate::session::load(id);
        self.chat_agent = None;
        self.oracle_busy = false;
        self.provider_context = None; // recomputed from BPE / the next run
        self.save_session();
    }

    /// Total tokens this session: (total, oracle, sub-agents). The oracle is the
    /// hidden chat session (no card); sub-agent cards sum into the rest.
    pub fn token_totals(&self) -> (u32, u32, u32) {
        let agents: u32 = self.agents.cards.iter().map(|c| c.tokens).sum();
        (self.oracle_tokens + agents, self.oracle_tokens, agents)
    }

    /// The CURRENT context size in tokens for the meter. Prefers the provider's
    /// EXACT count from the last run (cloud / cloud-principle local) and falls
    /// back to the cached REAL BPE count of the transcript (local / before the
    /// first response). Never the old chars/4 heuristic.
    pub fn context_tokens(&self) -> u32 {
        self.provider_context
            .filter(|&n| n > 0)
            .unwrap_or(self.context_tok_cache)
    }

    /// Recompute the BPE context count IFF the transcript/input changed. Cheap
    /// signature (Σ chars) gates the expensive re-encode; call it from a tick.
    /// The pending input line is included — that is what the next send replays.
    pub fn refresh_context_tokens(&mut self) {
        let input_len = self.chat.input.chars().count();
        let sig: usize = self
            .chat
            .msgs
            .iter()
            .map(|m| m.text.chars().count())
            .sum::<usize>()
            + input_len;
        if sig == self.context_tok_sig {
            return;
        }
        self.context_tok_sig = sig;
        let mut joined = String::new();
        for m in &self.chat.msgs {
            joined.push_str(&m.text);
            joined.push('\n');
        }
        joined.push_str(&self.chat.input);
        self.context_tok_cache = crate::tokens::count(&joined);
    }

    /// The effective model for new runs (the `/model` override, else the config
    /// default). Resolved WITHOUT `RunConfig::from_env` so the per-frame header
    /// read costs no `cortex.token` file read.
    pub fn current_model(&self) -> String {
        if let Some(m) = &self.model_override {
            return m.clone();
        }
        std::env::var("KEISEIKODE_MODEL").unwrap_or_else(|_| {
            if self.provider == "glm-zai" { "glm-4.6".into() } else { "sonnet".into() }
        })
    }

    /// Elapsed ms of the current oracle run — the spinner's clock.
    pub fn oracle_elapsed_ms(&self) -> u128 {
        self.oracle_started.elapsed().as_millis()
    }

    /// True while a TTS reply is (estimated to be) playing — the speaker's
    /// green state.
    pub fn is_speaking(&self) -> bool {
        self.speaking_until.map(|t| Instant::now() < t).unwrap_or(false)
    }

    /// A run whose role marks it as the ORACLE (the main chat session) rather
    /// than a sub-agent. Oracle runs never appear as agent cards.
    fn is_oracle_role(role: &str) -> bool {
        role == "chat" || role == "oracle"
    }

    /// Add `n` to the day counter, resetting it first if the local date rolled
    /// over (midnight). Feeds the "…/сут" figure in the token counter.
    fn bump_day(&mut self, n: u32) {
        let today = today_local();
        if today != self.day_stamp {
            self.day_stamp = today;
            self.day_tokens = 0;
        }
        self.day_tokens = self.day_tokens.saturating_add(n);
    }

    pub fn focus_next(&mut self) {
        self.focus = self.focus.next();
    }

    pub fn focus_prev(&mut self) {
        self.focus = self.focus.prev();
    }

    /// The agent id whose detail is expanded in the center, if any.
    pub fn open_agent_id(&self) -> Option<&str> {
        match &self.center {
            CenterMode::Agent(id) => Some(id.as_str()),
            _ => None,
        }
    }

    /// Esc: collapse a secondary center mode back to the primary Chat.
    pub fn close_center(&mut self) {
        self.center = CenterMode::Chat;
    }

    pub fn next_agent_label(&mut self) -> String {
        self.launched += 1;
        format!("glm-agent-{}", self.launched)
    }

    /// True when `id` is the run bound to the chat, so its live TEXT mirrors
    /// into the chat transcript. Tool ACTIONS never enter the chat — they live
    /// on the agent card in the right sidebar only.
    fn is_chat_agent(&self, id: &str) -> bool {
        self.chat_agent.as_deref() == Some(id)
    }

    /// Append streamed agent TEXT to the chat, coalescing onto the last agent
    /// bubble so a token stream reads as one growing message.
    fn chat_stream(&mut self, text: &str) {
        match self.chat.msgs.last_mut() {
            Some(m) if m.role == Role::Agent => m.text.push_str(text),
            _ => self.chat.push(Role::Agent, text.to_string()),
        }
    }

    /// Fold one live run event into the agents sidebar + its card log. The
    /// chat mirrors ONLY agent TEXT (Delta) + errors — tool calls / started /
    /// done stay on the card (they are the agent's ACTIONS, shown right).
    pub fn apply_run_event(&mut self, ev: RunEvent) {
        match ev {
            RunEvent::Started { id, label, role, task } => {
                if Self::is_oracle_role(&role) {
                    // The ORACLE (main chat) — bind it for streaming + steering,
                    // but NEVER surface it as an agent card. Remember its id so
                    // the activity stream can attribute sub-agents to this TUI.
                    self.oracle_runs.insert(id.clone());
                    self.chat_agent = Some(id);
                    self.oracle_busy = true;
                    self.oracle_started = Instant::now();
                } else {
                    // A sub-agent the oracle launched — this is what the agents
                    // sidebar shows. It does NOT hijack the chat stream. Reveal
                    // the right sidebar so a launched agent is actually visible
                    // (it is collapsed by default → otherwise the card is drawn
                    // into a zero-width pane and looks like "no agents ran").
                    self.right_collapsed = false;
                    self.agents.upsert(AgentCard {
                        id,
                        label,
                        role,
                        task,
                        status: AgentStatus::Running,
                        last_tool: None,
                        tokens: 0,
                        started: Instant::now(),
                        log: vec!["█ started".to_string()],
                    });
                }
            }
            RunEvent::Tool { id, name, phase, resource, added, removed } => {
                if phase == "start" {
                    // Short file name (basename) for the log line; keep the full
                    // path for click-to-open.
                    let short = resource.as_deref().map(|p| {
                        std::path::Path::new(p)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| p.to_string())
                    });
                    // Green +N / red −M diff counts, rendered by chat.rs.
                    let diff = match (added, removed) {
                        (Some(a), Some(r)) => format!("  +{a} -{r}"),
                        (Some(a), None) => format!("  +{a}"),
                        _ => String::new(),
                    };
                    if self.is_chat_agent(&id) {
                        // The ORACLE'S OWN actions stream into the chat as a
                        // process log — Claude-Code style: `● Tool(arg)` with a
                        // ● dot beside each tool (bash/read/edit/…). `resource`
                        // is the full arg (path/command/pattern); `short` still
                        // drives the sub-agent card branch below.
                        let _ = &short;
                        self.oracle_tool = Some(name.clone());
                        let cap = cap_tool(&name);
                        let line = match &resource {
                            Some(r) => format!("● {cap}({r}){diff}"),
                            None => format!("● {cap}{diff}"),
                        };
                        self.chat.push(Role::Tool, line);
                        // Remember an edited file so a click on the line opens it.
                        if matches!(name.as_str(), "edit" | "write" | "multiedit") {
                            if let Some(p) = &resource {
                                self.last_edit_path = Some(PathBuf::from(p));
                            }
                        }
                        // Sub-agent cards are driven by the activity stream
                        // (RunEvent::Activity) — real nested runs with a live
                        // current_step — not synthesized from this tool-use.
                    } else if let Some(c) = self.agents.cards.iter_mut().find(|c| c.id == id) {
                        // A sub-agent's actions stay on its sidebar card.
                        c.last_tool = Some(name.clone());
                        c.status = AgentStatus::Running;
                        let line = match &short {
                            Some(f) => format!("> tool: {name} {f}"),
                            None => format!("> tool: {name}"),
                        };
                        c.log.push(line);
                    }
                }
            }
            RunEvent::Delta { id, text } => {
                let words = text.split_whitespace().count().max(1) as u32;
                if self.is_chat_agent(&id) {
                    // Oracle text > the main chat (+ oracle token count). Text
                    // resuming means the tool finished — clear the activity dot.
                    self.oracle_tool = None;
                    self.chat_stream(&text);
                    self.oracle_tokens += words;
                    self.run_tokens += words;
                    self.bump_day(words);
                } else if let Some(c) = self.agents.cards.iter_mut().find(|c| c.id == id) {
                    // Sub-agent text > its card log only, never the main chat.
                    c.tokens += words;
                    match c.log.last_mut() {
                        Some(l) if l.starts_with("» ") => l.push_str(&text),
                        _ => c.log.push(format!("» {text}")),
                    }
                    if c.log.len() > 400 {
                        let excess = c.log.len() - 400;
                        c.log.drain(0..excess);
                    }
                    self.bump_day(words);
                }
            }
            RunEvent::Done { id } => {
                if self.chat_agent.as_deref() == Some(id.as_str()) {
                    self.oracle_busy = false;
                    self.oracle_tool = None;
                }
                if let Some(c) = self.agents.cards.iter_mut().find(|c| c.id == id) {
                    c.status = AgentStatus::Done;
                    c.log.push("✓ done".to_string());
                }
                // Voice reply: speak the agent's last chat message aloud (TTS).
                if self.voice_reply && self.is_chat_agent(&id) {
                    if let Some(text) = self
                        .chat
                        .msgs
                        .iter()
                        .rev()
                        .find(|m| m.role == Role::Agent && !m.text.trim().is_empty())
                        .map(|m| m.text.clone())
                    {
                        // Light the speaker green for roughly the spoken length
                        // (~14 chars/sec), clamped — the three-state icon.
                        let secs = (text.chars().count() as u64 / 14).clamp(2, 40);
                        self.speaking_until = Some(Instant::now() + std::time::Duration::from_secs(secs));
                        let cfg = RunConfig::from_env();
                        tokio::spawn(async move {
                            if let Ok(bytes) = crate::voice::fetch_tts(&cfg.base, &cfg.token, &text).await {
                                crate::voice::play(bytes);
                            }
                        });
                    }
                }
            }
            RunEvent::Usage { id, input, output } => {
                // Only the ORACLE (main chat) drives the context meter — sub-agent
                // runs report their own usage but the meter tracks the main
                // conversation. Use the provider's EXACT prompt tokens as the
                // context ONLY when sane: 0 means the provider sent no usage
                // frame, and a value above the window means a multi-turn tool
                // loop summed its per-turn prompts (the cortex accum sums). Both
                // fall back to the live BPE estimate.
                if self.is_chat_agent(&id) {
                    self.provider_context = if input > 0 && input <= MAX_CONTEXT_TOKENS {
                        Some(input)
                    } else {
                        None
                    };
                }
                // The provider's real output count replaces the live word-count
                // for THIS request's figure (exact per-request cost).
                if self.is_chat_agent(&id) && output > 0 {
                    self.run_tokens = output;
                }
            }
            RunEvent::Voice(text) => {
                // Mic transcript > drop into the chat input for review + Enter.
                if text.trim().is_empty() {
                    self.status = "voice: no speech recognized".into();
                } else {
                    self.chat.input = text;
                    self.status = "voice: transcript ready — press Enter to send".into();
                }
            }
            RunEvent::Error { id, msg } => {
                if self.chat_agent.as_deref() == Some(id.as_str()) {
                    self.oracle_busy = false;
                }
                if self.is_chat_agent(&id) {
                    self.chat.push(Role::Agent, format!("x error: {msg}"));
                }
                if let Some(c) = self.agents.cards.iter_mut().find(|c| c.id == id) {
                    c.status = AgentStatus::Error;
                    c.log.push(format!("x error: {msg}"));
                }
                self.status = format!("agent error: {msg}");
            }
            RunEvent::Activity(runs) => self.apply_activity(runs),
        }
    }

    /// Fold one cortex activity snapshot into the agents sidebar: each
    /// sub-agent whose `parent_id` is one of THIS TUI's oracle runs becomes a
    /// card, with its live `current_step` and terminal status. This is the real
    /// nested-agent view (replacing the tool-use-synthesized stopgap).
    fn apply_activity(&mut self, runs: Vec<crate::runs::RunView>) {
        for v in runs {
            if v.kind != "subagent" {
                continue;
            }
            if !v.parent_id.as_deref().is_some_and(|p| self.oracle_runs.contains(p)) {
                continue;
            }
            let status = match v.status.as_str() {
                "done" | "completed" => AgentStatus::Done,
                "failed" | "error" => AgentStatus::Error,
                _ => AgentStatus::Running,
            };
            let label = if v.model.is_empty() { "sub-agent".to_string() } else { v.model.clone() };
            if let Some(c) = self.agents.cards.iter_mut().find(|c| c.id == v.id) {
                c.status = status;
                // Live step text → the card's last-tool line + a log entry when
                // it changes (so the card shows what the sub-agent is doing now).
                if !v.current_step.is_empty() && c.last_tool.as_deref() != Some(v.current_step.as_str()) {
                    c.last_tool = Some(v.current_step.clone());
                    c.log.push(format!("> {}", v.current_step));
                    if c.log.len() > 400 {
                        let excess = c.log.len() - 400;
                        c.log.drain(0..excess);
                    }
                }
                if matches!(status, AgentStatus::Done | AgentStatus::Error)
                    && c.log.last().map(|l| l.as_str()) != Some("✓ done")
                {
                    c.log.push("✓ done".to_string());
                }
            } else {
                // First time we see this sub-agent — reveal the sidebar so it's
                // visible (it defaults to collapsed) and add its card.
                self.right_collapsed = false;
                let mut log = vec!["█ spawned".to_string()];
                if !v.current_step.is_empty() {
                    log.push(format!("> {}", v.current_step));
                }
                self.agents.upsert(AgentCard {
                    id: v.id.clone(),
                    label,
                    role: "sub-agent".into(),
                    task: v.model.clone(),
                    status,
                    last_tool: if v.current_step.is_empty() { None } else { Some(v.current_step.clone()) },
                    tokens: 0,
                    started: Instant::now(),
                    log,
                });
            }
        }
    }
}

/// Local calendar date `YYYY-MM-DD` (drives the midnight reset of `day_tokens`).
fn today_local() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Display name for a tool in the chat's `● Tool(arg)` line (Claude-Code style):
/// our lowercase tool ids → PascalCase labels. Unknown tools capitalize.
fn cap_tool(name: &str) -> String {
    let known = match name {
        "read" => "Read",
        "write" => "Write",
        "edit" | "multiedit" => "Edit",
        "bash" => "Bash",
        "grep" => "Grep",
        "glob" => "Glob",
        "web_search" => "WebSearch",
        "web_lookup" => "WebLookup",
        "webfetch" => "WebFetch",
        "agent" => "Agent",
        "todowrite" => "TodoWrite",
        "todoread" => "TodoRead",
        "create_agent" => "CreateAgent",
        other => {
            let mut c = other.chars();
            return match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            };
        }
    };
    known.to_string()
}
