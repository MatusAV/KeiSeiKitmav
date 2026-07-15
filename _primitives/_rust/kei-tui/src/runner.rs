//! Async event loop: crossterm key/mouse events + the live agent-run stream
//! drive the cockpit `App`. Everything is clickable — click a file to open it,
//! a folder to expand, an agent card to see its live detail, or empty agents
//! space to launch a new GLM agent. F5 also launches; Ctrl-Q quits.

use std::io::Write;
use std::time::Duration;

use crate::app::{App, CenterMode, LeftView};
use crate::chat::Role;
use crate::keys::{global, Global};
use crate::runs::{send_input, spawn_activity_stream, spawn_run, spawn_run_messages_image, RunConfig, RunEvent};
use crate::types::Pane;
use crate::ui::{draw, pane_at, regions};
use anyhow::Result;
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use futures::StreamExt;
use ratatui::backend::Backend;
use ratatui::layout::Rect;
use ratatui::Terminal;
use tokio::sync::mpsc::{self, UnboundedSender};

const DEMO_PROMPT: &str = "List the files and directories in the current working directory using \
your tools, then reply with how many entries there are.";

/// Card height in the agents pane (border + 2 body lines + 1 spacer ≈ 4 rows).
const CARD_ROWS: u16 = 4;

pub async fn run<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<RunEvent>();
    // One long-lived subscription to the cortex activity stream feeds the agents
    // sidebar the real nested sub-agents (filtered by parent_id in app.rs).
    spawn_activity_stream(app.run_config(), tx.clone());
    let mut events = EventStream::new();
    // Redraw ~7×/s so the spinning Frobenius sphere animates while an agent thinks.
    let mut ticker = tokio::time::interval(Duration::from_millis(140));
    apply_theme(); // push the active theme's colors to the host terminal
    // Startup splash: KEISEI > CODE > C morphs to K > KODE. Skip on any key or
    // via KEI_TUI_NOSPLASH (tests / headless).
    if std::env::var("KEI_TUI_NOSPLASH").is_err() {
        crate::splash::play(terminal).await;
    }
    let _ = write!(std::io::stdout(), "\x1b]2;kei-tui cockpit\x07"); // window title
    let _ = std::io::stdout().flush();
    if std::env::var("KEI_TUI_AUTOLAUNCH").is_ok() {
        launch_agent(&mut app, &tx);
    }
    // Throttle the day-total DB read (all sessions today) to ~5s, off the hot path.
    let mut last_day_read: Option<std::time::Instant> = None;
    while !app.should_quit {
        terminal.draw(|f| draw(f, &mut app))?;
        let area = terminal
            .size()
            .map(|s| Rect::new(0, 0, s.width, s.height))
            .unwrap_or(Rect::new(0, 0, 80, 24));
        tokio::select! {
            maybe = events.next() => {
                if let Some(Ok(ev)) = maybe {
                    handle_event(&mut app, ev, area, &tx);
                }
            }
            Some(re) = rx.recv() => app.apply_run_event(re),
            _ = ticker.tick() => {
                // t40: live re-read the passport when it's visible (pinned or
                // full-screen), so pinned task status tracks disk changes.
                if app.pin_passport || matches!(app.center, CenterMode::Plan) {
                    app.passport.maybe_reload();
                }
                // Real BPE context count — cheap signature check makes this a
                // no-op unless the transcript/input changed since last tick.
                app.refresh_context_tokens();
                // Refresh the day-total counter (every session today) at most
                // every ~5s — a tiny read of ~/.keisei/token-events.sqlite.
                let due = last_day_read.map(|t| t.elapsed().as_secs() >= 5).unwrap_or(true);
                if due {
                    last_day_read = Some(std::time::Instant::now());
                    let now = chrono::Local::now().timestamp();
                    app.day_all_tokens = crate::day_total::today_tokens(now);
                }
            }
        }
    }
    Ok(())
}

fn handle_event(app: &mut App, ev: Event, area: Rect, tx: &UnboundedSender<RunEvent>) {
    match ev {
        Event::Key(k) if k.kind != KeyEventKind::Release => handle_key(app, k, tx),
        Event::Mouse(m) => handle_mouse(app, m, area, tx),
        _ => {}
    }
}

fn handle_key(app: &mut App, k: KeyEvent, tx: &UnboundedSender<RunEvent>) {
    // Esc: close the editor first (> chat full), else collapse a secondary
    // center mode back to the primary Chat.
    if k.code == KeyCode::Esc {
        // In the DNA window mid-edit, Esc cancels the edit (not the window).
        if matches!(app.center, CenterMode::Dna) && app.dna.is_editing() {
            app.dna.cancel_edit();
            app.status = "DNA: edit cancelled".into();
            return;
        }
        // The command palette, if open, is the most local thing Esc cancels.
        // In a drill level Esc pops ONE window (step back up the tree); at the
        // root it closes the palette.
        if app.palette.open {
            if app.palette.in_drill() {
                app.palette.pop();
                app.status = "back".into();
            } else {
                app.palette.close();
                app.chat.input.clear();
                app.status = "commands closed".into();
            }
            return;
        }
        // A half-typed line is the most local thing Esc can cancel, so it goes
        // first: Esc discards it and closes nothing. Esc on an empty line then
        // walks the panes outward, as before.
        if app.chat.clear_input() {
            app.status = "input cleared".into();
            return;
        }
        // Esc also steps back off the mode bar, returning the caret to the chat.
        if app.bar_focus.is_some() {
            app.bar_focus = None;
            app.status = "back to chat".into();
            return;
        }
        if app.agents.is_expanded() {
            app.agents.collapse();
            app.status = "back to agents".into();
            return;
        }
        if app.editor.is_open() && app.editor_focus {
            // Editing > first press returns to the chat; the file stays open.
            app.editor_focus = false;
            app.status = "editor: back to chat (file still open)".into();
            return;
        }
        if app.editor.is_open() {
            app.editor.close();
            app.status = "editor closed".into();
            return;
        }
        // Close the file manager from ANY focus (a char-key can't reach the chat
        // to type /f while the tree has focus, so Esc must always work here).
        if !app.tree_collapsed {
            app.tree_collapsed = true;
            app.focus = Pane::Terminal;
            app.status = "files hidden (Esc)".into();
            return;
        }
        if !app.right_collapsed {
            app.right_collapsed = true;
            app.status = "agents hidden (Esc)".into();
            return;
        }
        if !matches!(app.center, CenterMode::Chat) {
            app.close_center();
            app.status = "back to chat".into();
            return;
        }
    }
    // P (no modifier): PIN the current full-screen window into its sidebar (or
    // unpin if already pinned), then return to the chat. Only fires when a
    // pinnable window fills the center — in the chat, P types normally.
    if k.code == KeyCode::Char('p') && !k.modifiers.contains(KeyModifiers::CONTROL) {
        match app.center {
            CenterMode::Plan => { toggle_pin_passport(app); return; }
            CenterMode::Agents => { toggle_pin_agents(app); return; }
            _ => {}
        }
        // The file tree is a left-pane window, not a center mode — P pins it when
        // the tree pane has focus.
        if app.focus == Pane::Tree {
            app.pin_tree = !app.pin_tree;
            app.tree_collapsed = !app.pin_tree;
            app.status = if app.pin_tree { "files pinned (P to unpin)".into() } else { "files unpinned".into() };
            return;
        }
    }
    // Ctrl-B: collapse/expand the left file-tree column (more room for chat).
    if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('b') {
        app.tree_collapsed = !app.tree_collapsed;
        app.status = if app.tree_collapsed { "tree collapsed (Ctrl-B)".into() } else { "tree expanded".into() };
        return;
    }
    // Ctrl-L: collapse/expand the RIGHT column (agents + passport) > wider chat.
    if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('l') {
        app.right_collapsed = !app.right_collapsed;
        app.status = if app.right_collapsed {
            "right sidebar collapsed (Ctrl-L)".into()
        } else {
            "right sidebar expanded".into()
        };
        return;
    }
    // Ctrl-P: open the project plan / passport full-screen on the main center.
    if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('p') {
        show_plan(app);
        return;
    }
    // Ctrl-E: open the file the oracle most recently edited in the top editor.
    if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('e') {
        if let Some(p) = app.last_edit_path.clone() {
            app.editor.open(p.clone());
            app.center = CenterMode::Chat;
            app.editor_focus = false;
            app.status = format!("opened {} (Ctrl-E) · F6 to edit", p.display());
        } else {
            app.status = "no edited file yet".into();
        }
        return;
    }
    // Ctrl-S: save the open file.
    if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('s') {
        if app.editor.is_open() {
            app.status = match app.editor.save() {
                Ok(p) => format!("saved {}", p.display()),
                Err(e) => format!("save failed: {e}"),
            };
        }
        return;
    }
    // When a file is open AND editing is focused (F6 / clicked), keystrokes edit
    // the code instead of the chat. Global shortcuts above still win.
    if app.editor.is_open() && app.editor_focus {
        app.editor.on_key(k.code);
        return;
    }
    // Ctrl-Space (or F2): push-to-talk — toggle mic recording. On stop the WAV
    // is transcribed by the cortex STT endpoint and the transcript lands in the
    // chat input (RunEvent::Voice). Ctrl-Space is unreliable across terminals
    // (often arrives as a NUL), so F2 is a robust alias.
    let ptt = (k.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(k.code, KeyCode::Char(' ') | KeyCode::Char('\0')))
        || k.code == KeyCode::F(2);
    if ptt {
        toggle_record(app, tx);
        return;
    }
    // Ctrl-R (or F7): toggle voice-reply — speak the agent's answer aloud (TTS).
    if (k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('r'))
        || k.code == KeyCode::F(7)
    {
        app.voice_reply = !app.voice_reply;
        app.status = if app.voice_reply {
            "voice reply ON — Keisei speaks answers (Ctrl-R/F7)".into()
        } else {
            "voice reply OFF (Ctrl-R/F7)".into()
        };
        return;
    }
    // Center-mode switches. Handled before `global`/focus so the chat input
    // can't swallow them.
    match k.code {
        KeyCode::F(9) => {
            app.left_view = match app.left_view {
                LeftView::Files => LeftView::Structure,
                LeftView::Structure => LeftView::Files,
            };
            app.status = "left: toggled files/structure (F9)".into();
            return;
        }
        KeyCode::F(4) => {
            app.center = match app.center {
                CenterMode::Terminal => CenterMode::Chat,
                _ => CenterMode::Terminal,
            };
            app.focus = Pane::Terminal;
            return;
        }
        KeyCode::F(6) => {
            if app.editor.is_open() {
                // Toggle editing focus on an already-open file.
                app.editor_focus = !app.editor_focus;
                app.status = if app.editor_focus {
                    "editing — type to edit · Ctrl-S save · Esc done".into()
                } else {
                    "editor: viewing (F6 to edit)".into()
                };
            } else {
                open_selected_in_editor(app);
                app.editor_focus = true;
            }
            return;
        }
        KeyCode::F(8) => {
            app.center = match app.center {
                CenterMode::Settings => CenterMode::Chat,
                _ => CenterMode::Settings,
            };
            app.focus = Pane::Terminal;
            return;
        }
        _ => {}
    }
    // When focus is DOWN in the mode bar, arrows navigate it + Enter toggles.
    if app.bar_focus.is_some() {
        bar_nav_key(app, k, tx);
        return;
    }
    if let Some(g) = global(k) {
        match g {
            Global::Quit => app.should_quit = true,
            Global::FocusNext => app.focus_next(),
            Global::Launch => launch_agent(app, tx),
            Global::Theme => {
                crate::theme::cycle();
                apply_theme();
                app.status = format!("theme: {} (F3 to cycle)", crate::theme::name());
            }
        }
        return;
    }
    match app.focus {
        // The "terminal" focus slot drives whichever widget owns the center.
        Pane::Terminal => match &app.center {
            CenterMode::Terminal => app.term.on_key(k),
            CenterMode::Settings => {
                // On the "you side" row, Enter / <> flips the chat alignment.
                if app.settings.selected() == crate::settings::YOU_SIDE_ROW
                    && matches!(k.code, KeyCode::Enter | KeyCode::Left | KeyCode::Right | KeyCode::Char(' '))
                {
                    app.user_right = !app.user_right;
                    app.status = format!("you on the {}", if app.user_right { "right" } else { "left" });
                } else {
                    app.settings.on_key(k.code);
                }
            }
            // Agents dashboard: F5 launches, Ctrl-P opens the plan (both handled
            // by the global keys above); Esc returns to chat.
            // These full-screen views are view-only under the terminal focus —
            // the global Esc closes them back to the chat.
            CenterMode::Agent(_) | CenterMode::Plan | CenterMode::Agents | CenterMode::Image(_) => {}
            // DNA window: ↑↓ move the field cursor; Enter begins editing the
            // selected scalar (or commits when already editing); typing edits the
            // value; Esc cancels an edit (the global Esc closes the window when
            // not editing).
            CenterMode::Dna => dna_key(app, k),
            // Chat is primary: typing > chat input; arrows scroll the editor
            // above it when a file is open.
            CenterMode::Chat => chat_key(app, k, tx),
        },
        Pane::Tree => match k.code {
            // < or Esc closes the file manager and returns to the chat.
            KeyCode::Left | KeyCode::Esc => {
                app.tree_collapsed = true;
                app.focus = Pane::Terminal;
                app.status = "files hidden".into();
            }
            KeyCode::Tab => app.focus_next(),
            KeyCode::BackTab => app.focus_prev(),
            KeyCode::Enter => activate_tree(app),
            other => app.tree.on_key(other),
        },
        Pane::Agents => match k.code {
            KeyCode::Tab => app.focus_next(),
            KeyCode::BackTab => app.focus_prev(),
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Char('a') => launch_agent(app, tx),
            KeyCode::Enter => {
                // Stage 1: expand the card IN the sidebar. Stage 2: a second
                // Enter (already expanded) opens it in the big center chat.
                if let Some(id) = app.agents.expanded_id() {
                    app.agents.collapse();
                    app.center = CenterMode::Agent(id);
                    app.focus = Pane::Terminal;
                    app.status = "agent opened in chat — Esc back".into();
                } else {
                    app.agents.expand_selected();
                    app.status = "agent expanded — Enter > open in chat · Esc > back".into();
                }
            }
            other => app.agents.on_key(other),
        },
    }
}

/// Route a keypress while the primary Chat owns the center. Typing edits the
/// chat input; Enter submits; the arrow/page keys scroll the code editor when
/// one is riding above the chat.
fn chat_key(app: &mut App, k: KeyEvent, tx: &UnboundedSender<RunEvent>) {
    // While the command palette is open it OWNS the keyboard: arrows move the
    // selection, Enter picks, Esc closes, typing filters, Backspace narrows (and
    // closes when it eats the leading slash). This is what turns `/command` into
    // a navigable pop-up instead of dumping help text into the chat.
    if app.palette.open {
        match k.code {
            KeyCode::Up => app.palette.move_up(),
            KeyCode::Down => app.palette.move_down(),
            KeyCode::Esc => {
                app.palette.close();
                app.chat.input.clear();
            }
            KeyCode::Enter | KeyCode::Tab => palette_pick(app, tx),
            KeyCode::Backspace => {
                if app.palette.backspace() {
                    app.chat.backspace(); // keep the visible input in sync
                } else {
                    // Backspace ate the leading slash → close + clear.
                    app.palette.close();
                    app.chat.input.clear();
                }
            }
            KeyCode::Char(c) => {
                app.chat.on_char(c);
                app.palette.push(c);
            }
            _ => {}
        }
        return;
    }
    match k.code {
        // Shift-Tab flips the approval toggle: auto ↔ accept-edits.
        KeyCode::BackTab => {
            app.approval_auto = !app.approval_auto;
            app.touch_bar();
            app.status = format!("approval: {}", if app.approval_auto { "auto" } else { "accept-edits" });
        }
        // `/` on an EMPTY input opens the command palette (Claude-Code / Grok).
        KeyCode::Char('/') if app.chat.input.is_empty() => {
            app.chat.on_char('/');
            app.palette.open();
            app.status = "commands — ↑↓ pick · Enter run · Esc close".into();
        }
        KeyCode::Char(c) => { app.recall_idx = 0; app.chat.on_char(c); }
        KeyCode::Backspace => { app.recall_idx = 0; app.chat.backspace(); }
        KeyCode::Enter => { app.recall_idx = 0; chat_send(app, tx); }
        // PageUp/PageDown always scroll the chat history.
        KeyCode::PageUp => app.chat.scroll_up(10),
        KeyCode::PageDown => app.chat.scroll_down(10),
        // Arrows drive the editor cursor when a file is open, else: Up from the
        // input is shell-style command RECALL (not scroll) — Up#1 pulls the last
        // user message into the input, Up#2 opens the full history window; Down
        // Down scrolls the chat. (It no longer descends into the mode bar —
        // every mode toggle is reachable from the /command palette instead.)
        code @ (KeyCode::Up | KeyCode::Down) => {
            if app.editor.is_open() {
                app.editor.on_key(code);
            } else if code == KeyCode::Up {
                recall_up(app);
            } else {
                app.chat.scroll_down(1);
            }
        }
        _ => {}
    }
}

/// Enter in the palette. Two cases: (1) inside a DRILL level, act on the
/// selected row (load session / set model / open agent / open DNA / recall);
/// (2) at the ROOT, either drill into a sub-list (sessions/models/agents/…) or
/// run/prefill a plain command.
fn palette_pick(app: &mut App, tx: &UnboundedSender<RunEvent>) {
    use crate::palette::RowAction;
    // (1) A drill level is on screen → act on its selected row.
    if app.palette.in_drill() {
        let Some(act) = app.palette.current_row_action() else {
            app.palette.close();
            app.chat.input.clear();
            return;
        };
        // SETTINGS-style rows apply the change and KEEP the window open (Esc
        // closes) — click through models/mic/speak/effort/theme like a settings
        // pane. NAVIGATION rows act and close.
        let stays = act.stays_open();
        match act {
            RowAction::LoadSession(id) => {
                app.load_session(&id);
                app.status = format!("loaded session {id}");
            }
            RowAction::SetModel(m) => apply_model_choice(app, &m),
            RowAction::SetEffort(e) => {
                app.effort = e.clone();
                app.status = format!("effort > {e}");
            }
            RowAction::SetTheme(_) => {
                crate::theme::cycle();
                apply_theme();
                app.status = format!("theme: {}", crate::theme::name());
            }
            RowAction::ToggleMic => toggle_record(app, tx),
            RowAction::ToggleSpeak => {
                app.voice_reply = !app.voice_reply;
                app.status = format!("speak replies: {}", if app.voice_reply { "on" } else { "off" });
            }
            RowAction::SetApproval(mode) => {
                app.approval_auto = mode == "auto";
                app.status = format!("approval: {mode}");
            }
            RowAction::OpenAgent(id) => {
                app.center = CenterMode::Agent(id);
                app.focus = Pane::Terminal;
            }
            RowAction::OpenDna(name) => open_dna_view(app, &name),
            RowAction::Recall(cmd) => {
                app.chat.input = cmd;
                app.palette.close();
                return; // leave the recalled text in the input for the user
            }
        }
        if stays {
            // Rebuild the level in place so the ✓ tracks the new active choice,
            // preserving the cursor position.
            rebuild_active_level(app);
            return;
        }
        app.palette.close();
        app.chat.input.clear();
        return;
    }

    // (2) At the root command list.
    let Some(cmd) = app.palette.current() else {
        app.palette.close();
        return;
    };
    // Commands that DRILL into a sub-list instead of running flatly.
    match cmd.name {
        "sessions" => { drill_sessions(app); return; }
        "model" => { drill_models(app); return; }
        "effort" => { drill_effort(app); return; }
        "mic" => { drill_voice(app); return; }
        "speak" => { drill_voice(app); return; }
        "agents" => { drill_agents_live(app); return; }
        "agentslib" => { drill_agents_lib(app); return; }
        _ => {}
    }
    app.palette.close();
    match cmd.kind {
        crate::palette::CmdKind::Run => {
            app.chat.input.clear();
            handle_slash(app, &format!("/{}", cmd.name), tx);
        }
        crate::palette::CmdKind::NeedsArg => {
            app.chat.input = format!("/{} ", cmd.name);
            app.status = format!("/{} … type the argument, Enter to run", cmd.name);
        }
    }
}

/// Push the SESSIONS drill level: every saved session (id + preview) → load.
fn drill_sessions(app: &mut App) {
    use crate::palette::{DynRow, Level, RowAction};
    let rows: Vec<DynRow> = crate::session::list()
        .into_iter()
        .take(50)
        .map(|s| DynRow {
            label: s.id.clone(),
            hint: s.preview.chars().take(48).collect(),
            action: RowAction::LoadSession(s.id),
            active: false,
        })
        .collect();
    app.palette.drill(Level::new("sessions", rows));
    app.status = "sessions — ↑↓ pick · Enter load · Esc back".into();
}

/// The known models the cockpit ships with (name, hint). Single source used by
/// both the drill list and its rebuild.
const MODELS: &[(&str, &str)] = &[
    ("glm-4.6", "GLM · z.ai (tools, streaming)"),
    ("glm-5.2", "GLM · z.ai (larger)"),
    ("sonnet", "Claude · subscription (claude -p)"),
    ("opus", "Claude · subscription (max effort)"),
    ("haiku", "Claude · subscription (fast)"),
];

/// Build the MODELS level's rows, marking the active one. Selecting a model
/// keeps the window open (settings-style) — the ✓ moves to the picked model.
fn models_rows(app: &App) -> Vec<crate::palette::DynRow> {
    use crate::palette::{DynRow, RowAction};
    let cur = app.model_override.clone().unwrap_or_default();
    MODELS
        .iter()
        .map(|(m, h)| DynRow {
            label: format!("/{m}"),
            hint: h.to_string(),
            action: RowAction::SetModel(m.to_string()),
            active: *m == cur,
        })
        .collect()
}

/// Push the MODELS drill level (settings-style: pick keeps it open).
fn drill_models(app: &mut App) {
    let rows = models_rows(app);
    app.palette.drill(crate::palette::Level::new("model", rows));
    app.status = "model — ↑↓ pick · Enter set · Esc close".into();
}

/// Build the EFFORT level's rows (low/medium/high), marking the active one.
fn effort_rows(app: &App) -> Vec<crate::palette::DynRow> {
    use crate::palette::{DynRow, RowAction};
    ["low", "medium", "high"]
        .iter()
        .map(|e| DynRow {
            label: e.to_string(),
            hint: String::new(),
            action: RowAction::SetEffort(e.to_string()),
            active: app.effort == *e,
        })
        .collect()
}

/// Push the EFFORT drill level (settings-style).
fn drill_effort(app: &mut App) {
    let rows = effort_rows(app);
    app.palette.drill(crate::palette::Level::new("effort", rows));
    app.status = "effort — ↑↓ pick · Enter set · Esc close".into();
}

/// Build the VOICE level's rows: mic on/off + speak on/off, marking active.
fn voice_rows(app: &App) -> Vec<crate::palette::DynRow> {
    use crate::palette::{DynRow, RowAction};
    vec![
        DynRow {
            label: "microphone".into(),
            hint: if app.recorder.is_some() { "recording" } else { "off" }.into(),
            action: RowAction::ToggleMic,
            active: app.recorder.is_some(),
        },
        DynRow {
            label: "speak replies".into(),
            hint: if app.voice_reply { "on" } else { "off" }.into(),
            action: RowAction::ToggleSpeak,
            active: app.voice_reply,
        },
    ]
}

/// Push the VOICE drill level (settings-style: toggles keep it open).
fn drill_voice(app: &mut App) {
    let rows = voice_rows(app);
    app.palette.drill(crate::palette::Level::new("voice", rows));
    app.status = "voice — ↑↓ pick · Enter toggle · Esc close".into();
}

/// After a settings-style pick, rebuild the current level's rows in place so the
/// ✓ tracks the new active choice — preserving the cursor position. Keyed off
/// the level title (the only levels that stay open are these settings ones).
fn rebuild_active_level(app: &mut App) {
    let Some(title) = app.palette.stack.last().map(|l| l.title.clone()) else { return };
    let rows = match title.as_str() {
        "model" => models_rows(app),
        "effort" => effort_rows(app),
        "voice" => voice_rows(app),
        _ => return,
    };
    if let Some(lvl) = app.palette.stack.last_mut() {
        let sel = lvl.selected.min(rows.len().saturating_sub(1));
        lvl.rows = rows;
        lvl.selected = sel;
    }
}

/// Push the LIVE AGENTS drill level: this session's child agents, then a
/// divider, then the other active runs. Enter opens the agent full-screen.
fn drill_agents_live(app: &mut App) {
    use crate::palette::{DynRow, Level, RowAction};
    let mut rows: Vec<DynRow> = Vec::new();
    // Child agents of THIS session go on top.
    for c in &app.agents.cards {
        rows.push(DynRow {
            label: format!("● {}", c.label),
            hint: c.task.chars().take(40).collect(),
            action: RowAction::OpenAgent(c.id.clone()),
            active: false,
        });
    }
    if rows.is_empty() {
        rows.push(DynRow { label: "(no live agents)".into(), hint: "F5 launches one".into(), action: RowAction::Recall(String::new()), active: false });
    }
    app.palette.drill(Level::new("agents · live", rows));
    app.status = "live agents — ↑↓ pick · Enter open · Esc back".into();
}

/// Push the AGENT LIBRARY drill level: the kit's registered agents (roster) →
/// open the DNA view. Populated from the on-disk manifests.
fn drill_agents_lib(app: &mut App) {
    use crate::palette::{DynRow, Level, RowAction};
    let rows: Vec<DynRow> = agent_roster()
        .into_iter()
        .map(|name| DynRow {
            label: name.clone(),
            hint: "view / edit DNA".into(),
            action: RowAction::OpenDna(name),
            active: false,
        })
        .collect();
    app.palette.drill(Level::new("agents · library", rows));
    app.status = "agent library — ↑↓ pick · Enter open DNA · Esc back".into();
}

/// Open the DNA view for a library agent: read its manifest TOML from disk and
/// show it full-screen (CenterMode::Dna). View + edit live there.
fn open_dna_view(app: &mut App, name: &str) {
    app.dna.open(name);
    app.center = CenterMode::Dna;
    app.focus = Pane::Terminal;
    app.status = format!("DNA: {name} — ↑↓ field · Enter edit · Esc back");
}

/// The kit's agent roster = the manifest stems on disk
/// (`~/work/KeiSeiKit-1.0/_manifests/*.toml`). Read directly (tui is on the same
/// host); falls back to an empty list if the dir is unreadable.
fn agent_roster() -> Vec<String> {
    let dir = "/home/keisei/work/KeiSeiKit-1.0/_manifests";
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let p = e.path();
                    if p.extension().and_then(|x| x.to_str()) == Some("toml") {
                        p.file_stem().map(|s| s.to_string_lossy().into_owned())
                    } else {
                        None
                    }
                })
                .filter(|n| !n.starts_with('_'))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Apply a model choice from the palette: map the well-known aliases to their
/// provider (mirrors the `/model` typed path), else set the raw model name.
fn apply_model_choice(app: &mut App, m: &str) {
    let a = m.to_lowercase();
    if a == "claude" || a == "opus" || a == "sonnet" || a == "haiku" {
        app.provider = "claude".into();
        app.model_override = Some(if a == "claude" { "sonnet".into() } else { a });
    } else if a.starts_with("glm") {
        app.provider = "glm-zai".into();
        app.model_override = Some(m.to_string());
    } else {
        app.model_override = Some(m.to_string());
    }
    let mm = app.model_override.clone().unwrap_or_default();
    app.status = format!("model > {mm} · {}", app.provider);
}

/// Shell-style Up-arrow command recall from the chat input. First press pulls
/// the most recent user message into the input; a second press (recall already
/// active) opens the full history WINDOW (a palette drill level) where you pick
/// any past command with ↑↓ + Enter.
fn recall_up(app: &mut App) {
    let hist = app.user_history();
    if hist.is_empty() {
        app.status = "no history yet".into();
        return;
    }
    if app.recall_idx == 0 {
        // First Up: recall the most recent entry into the input.
        app.recall_idx = 1;
        app.chat.input = hist[0].clone();
        app.status = "recall — Up again for the full list".into();
    } else {
        // Second Up: open the history window (palette drill), newest first.
        use crate::palette::{DynRow, Level, RowAction};
        let rows: Vec<DynRow> = hist
            .iter()
            .take(50)
            .map(|h| DynRow {
                // First line only in the list; full command on select.
                label: h.lines().next().unwrap_or("").chars().take(56).collect(),
                hint: String::new(),
                action: RowAction::Recall(h.clone()),
                active: false,
            })
            .collect();
        app.palette.open();
        app.palette.drill(Level::new("history", rows));
        app.recall_idx = 0;
        app.status = "history — ↑↓ pick · Enter to input · Esc close".into();
    }
}

/// P in the passport window: pin it into the right sidebar (or unpin). Pinning
/// returns to the chat with the passport now docked; the right sidebar shows
/// only what is pinned.
fn toggle_pin_passport(app: &mut App) {
    app.pin_passport = !app.pin_passport;
    app.right_collapsed = !(app.pin_passport || app.pin_agents);
    if app.pin_passport {
        app.close_center(); // back to chat; passport lives in the sidebar now
        app.status = "passport pinned to the sidebar (P to unpin)".into();
    } else {
        app.status = "passport unpinned".into();
    }
}

/// P in the agents dashboard: pin it into the right sidebar (or unpin).
fn toggle_pin_agents(app: &mut App) {
    app.pin_agents = !app.pin_agents;
    app.right_collapsed = !(app.pin_passport || app.pin_agents);
    if app.pin_agents {
        app.close_center();
        app.status = "agents pinned to the sidebar (P to unpin)".into();
    } else {
        app.status = "agents unpinned".into();
    }
}

/// Keys for the DNA window. Not editing: ↑↓ move the field cursor, Enter starts
/// editing the selected scalar. Editing: typing mutates the value, Enter commits
/// (writes the manifest), Esc cancels (the global Esc closes the window only
/// when NOT editing — see handle_key).
fn dna_key(app: &mut App, k: KeyEvent) {
    if app.dna.is_editing() {
        match k.code {
            KeyCode::Char(c) => app.dna.edit_char(c),
            KeyCode::Backspace => app.dna.edit_backspace(),
            KeyCode::Enter => match app.dna.commit_edit() {
                Ok(key) => app.status = format!("DNA: {key} saved"),
                Err(e) => app.status = format!("DNA save failed: {e}"),
            },
            KeyCode::Esc => {
                app.dna.cancel_edit();
                app.status = "DNA: edit cancelled".into();
            }
            _ => {}
        }
        return;
    }
    match k.code {
        KeyCode::Up => app.dna.move_up(),
        KeyCode::Down => app.dna.move_down(),
        KeyCode::Enter => {
            app.dna.begin_edit();
            app.status = "DNA: editing — type · Enter save · Esc cancel".into();
        }
        _ => {}
    }
}

/// Navigate the mode bar when focus is down in it (`bar_focus`). Items:
/// 0 auto · 1 accept-edits · 2 plan · 3 mic · 4 speaker.
fn bar_nav_key(app: &mut App, k: KeyEvent, tx: &UnboundedSender<RunEvent>) {
    let i = app.bar_focus.unwrap_or(0);
    match k.code {
        // Edge affordance: Left at the leftmost opens the file manager; Right at
        // the rightmost opens the agents panel.
        KeyCode::Left => {
            if i == 0 {
                app.tree_collapsed = false;
                app.left_view = LeftView::Files;
                app.status = "files opened (<)".into();
            } else {
                app.bar_focus = Some(i - 1);
            }
        }
        KeyCode::Right => {
            if i >= 3 {
                app.right_collapsed = false;
                app.status = "agents opened (>)".into();
            } else {
                app.bar_focus = Some(i + 1);
            }
        }
        KeyCode::Up | KeyCode::Esc => {
            app.bar_focus = None;
            app.status = "back to chat".into();
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            app.touch_bar();
            match i {
            // 0 approval toggle · 1 plan toggle · 2 mic · 3 speaker
            0 => {
                app.approval_auto = !app.approval_auto;
                app.status = format!("approval: {}", if app.approval_auto { "auto" } else { "accept-edits" });
            }
            1 => {
                app.plan_mode = !app.plan_mode;
                app.status = format!("plan mode: {}", if app.plan_mode { "on" } else { "off" });
            }
            2 => toggle_record(app, tx),
            _ => {
                app.voice_reply = !app.voice_reply;
                app.status = format!("speak replies: {}", if app.voice_reply { "on" } else { "off" });
            }
            }
        }
        _ => {}
    }
}

/// Submit the chat input. `/cp <project>` loads that project's `.mf` passport
/// into the right sidebar (t40); otherwise the line LAUNCHES a new agent when
/// none is live, or STEERS the live one via kei-cortex `/input` (t24).
fn chat_send(app: &mut App, tx: &UnboundedSender<RunEvent>) {
    let Some(line) = app.chat.take_input() else {
        return;
    };
    // Slash commands (opencode-style) are handled locally, never sent to a run.
    if line.starts_with('/') && handle_slash(app, &line, tx) {
        return;
    }
    app.chat.push(Role::User, line.clone());
    app.save_session();

    // PLAN mode: write the message as a `.mf` plan node into the project
    // passport instead of launching an agent (plan > passport.mf).
    if app.plan_mode {
        match write_plan_node(app, &line) {
            Ok(path) => {
                app.chat.push(Role::Agent, format!("plan node written > {path}"));
                app.status = format!("plan > {path}");
            }
            Err(e) => app.chat.push(Role::Agent, format!("x plan write failed: {e}")),
        }
        if let Some(p) = app.passport.project.clone() {
            app.passport.load(&p);
        }
        app.save_session();
        return;
    }

    if app.oracle_busy {
        // The oracle is mid-run > STEER it via kei-cortex /input (no new run).
        let id = app.chat_agent.clone().unwrap_or_default();
        let text = line;
        let cfg = app.run_config();
        tokio::spawn(async move {
            let _ = send_input(&cfg, &id, &text).await;
        });
        app.status = "> steered the oracle".into();
    } else {
        // Start a fresh ORACLE run carrying the WHOLE transcript so it has
        // memory of the conversation (role "chat" > hidden, no agent card).
        let msgs = app.chat.msgs.clone();
        let label = "oracle".to_string();
        let task: String = line.chars().take(48).collect();
        // Consume any image attached via drag-drop (vision) for this send.
        let image = app.pending_image.take();
        let attached = image.is_some();
        spawn_run_messages_image(app.run_config(), msgs, label, "chat".to_string(), task, tx.clone(), image);
        app.oracle_busy = true;
        app.oracle_started = std::time::Instant::now();
        // Drop the last run's exact context so the meter tracks the live BPE
        // estimate while this run grows, then snaps back to exact on completion.
        app.provider_context = None;
        app.run_tokens = 0; // per-request token counter starts fresh
        app.status = if attached {
            "oracle thinking (with image · glm-4.6v)".into()
        } else {
            "oracle thinking (memory · 0 Claude)".into()
        };
    }
}

/// Command list shown by `/help`.
const HELP_TEXT: &str = "commands:\n\
  /help                — this list\n\
  /f  /files           — toggle the file manager (left)\n\
  /a  /agents          — toggle the agents panel (right)\n\
  /ps /plan            — show the project plan on the main screen (Ctrl-P)\n\
  /cp <project>        — adopt a project's oracle + hang its tasks (right)\n\
  /new                 — start a fresh chat session\n\
  /sessions [id]       — list / load saved sessions\n\
  /model <name>        — set the model for new runs\n\
  /compact             — summarise + shrink the chat\n\
  /tools /skills       — the agent arsenal · our skill library\n\
  /model claude|glm  /effort low|med|high\n\
  /mic on|off  /mo /moff    — microphone (push-to-talk also F2)\n\
  /speak on|off /so /soff   — speak replies aloud (also F7)\n\
keys: Shift-Tab mode · Ctrl-B files · Ctrl-L agents · Ctrl-P plan · F5 launch · Ctrl-Q quit";

/// Handle a `/command`. Returns true if it was a slash command (recognized OR
/// not) so the line is never sent to a run — an unknown one gets a hint.
fn handle_slash(app: &mut App, line: &str, tx: &UnboundedSender<RunEvent>) -> bool {
    let mut it = line.trim().splitn(2, char::is_whitespace);
    let cmd = it.next().unwrap_or("");
    let arg = it.next().unwrap_or("").trim();
    match cmd {
        "/help" => app.chat.push(Role::Agent, HELP_TEXT.to_string()),
        // Sidebars: hidden by default, toggled on demand.
        "/f" | "/file" | "/files" => {
            app.tree_collapsed = !app.tree_collapsed;
            app.left_view = crate::app::LeftView::Files;
            if app.tree_collapsed {
                app.focus = Pane::Terminal;
                app.status = "files hidden".into();
            } else {
                // Focus the tree so ↑/↓ navigate it immediately; </Esc closes.
                app.focus = Pane::Tree;
                app.status = "files — ↑↓ navigate · Enter open · < / Esc close".into();
            }
        }
        "/a" | "/agent" | "/agents" => {
            // Agents now live in the CENTER dashboard (Claude-Code style), not
            // the right sidebar.
            app.center = match app.center {
                CenterMode::Agents => CenterMode::Chat,
                _ => CenterMode::Agents,
            };
            app.focus = Pane::Terminal;
            app.status = "agents dashboard — Esc back · F5 launch · Ctrl-P plan".into();
        }
        "/ps" | "/plan" => show_plan(app),
        "/terminal" | "/term" => {
            app.center = match app.center {
                CenterMode::Terminal => CenterMode::Chat,
                _ => CenterMode::Terminal,
            };
            app.focus = Pane::Terminal;
            app.status = "terminal (F4 to toggle · Esc back to chat)".into();
        }
        "/settings" => {
            app.center = match app.center {
                CenterMode::Settings => CenterMode::Chat,
                _ => CenterMode::Settings,
            };
            app.focus = Pane::Terminal;
            app.status = "settings (F8 to toggle · Esc back)".into();
        }
        "/tools" => {
            // The kei-cortex agent's arsenal (what the oracle can DO).
            app.chat.push(Role::Agent, "kei-cortex agent tools:\n  read · write · edit — files\n  bash — shell (timeout, workdir)\n  glob · grep — search\n  webfetch · web_search · web_lookup — the web\n  agent · create_agent — spawn sub-agents\n  todoread · todowrite — task list\nThe oracle uses these automatically; just ask.".into());
        }
        "/skills" => {
            // Our skill library under ~/.claude/skills/.
            let dir = std::env::var("HOME").ok().map(|h| format!("{h}/.claude/skills"));
            let mut s = String::from("skills (~/.claude/skills):\n");
            let mut names: Vec<String> = dir
                .and_then(|d| std::fs::read_dir(d).ok())
                .map(|rd| rd.flatten().filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().into_owned()).collect())
                .unwrap_or_default();
            names.sort();
            if names.is_empty() {
                s.push_str("  (none found)");
            } else {
                for chunk in names.chunks(6) {
                    s.push_str("  ");
                    s.push_str(&chunk.join(" · "));
                    s.push('\n');
                }
                s.push_str(&format!("({} skills) — ask the oracle to use one", names.len()));
            }
            app.chat.push(Role::Agent, s);
        }
        // Voice, explicit on/off (F2/F7 are the toggles).
        "/mo" | "/mic" if arg != "off" => {
            if app.recorder.is_none() { toggle_record(app, tx); }
        }
        "/moff" => {
            if app.recorder.is_some() { toggle_record(app, tx); }
        }
        "/mic" => {
            // "/mic off" reaches here (arg == "off"); "/mic on" handled above.
            if arg == "off" && app.recorder.is_some() { toggle_record(app, tx); }
            else if arg != "off" && app.recorder.is_none() { toggle_record(app, tx); }
        }
        "/so" => { app.voice_reply = true; app.status = "speak replies: on".into(); }
        "/soff" => { app.voice_reply = false; app.status = "speak replies: off".into(); }
        "/speak" => {
            app.voice_reply = arg != "off";
            app.status = format!("speak replies: {}", if app.voice_reply { "on" } else { "off" });
        }
        "/new" => {
            app.new_session();
            app.status = "new session started (/new)".into();
        }
        "/sessions" => {
            if arg.is_empty() {
                let ls = crate::session::list();
                let mut s = String::from("saved sessions (/sessions <id> to load):\n");
                if ls.is_empty() {
                    s.push_str("  (none yet)\n");
                }
                for si in ls.iter().take(20) {
                    s.push_str(&format!("  {} — {}\n", si.id, si.preview));
                }
                app.chat.push(Role::Agent, s);
            } else {
                app.load_session(arg);
                app.status = format!("loaded session {arg}");
            }
        }
        "/m" | "/model" => {
            if arg.is_empty() {
                let m = app.model_override.clone().unwrap_or_else(|| "glm-4.7".into());
                app.chat.push(Role::Agent, format!(
                    "model: {m} · provider: {}\n  /model claude  > Claude subscription (claude -p)\n  /model glm     > GLM (z.ai)\n  /model <name>  > any model on the current provider",
                    app.provider
                ));
            } else {
                let a = arg.to_lowercase();
                // Provider-switching aliases; else just set the model name.
                if a == "claude" || a == "opus" || a == "sonnet" || a == "haiku" {
                    app.provider = "claude".into();
                    app.model_override = Some(if a == "claude" { "sonnet".into() } else { a });
                } else if a == "glm" {
                    app.provider = "glm-zai".into();
                    // 4.6 answers on the z.ai anthropic endpoint; 4.7 does not.
                    app.model_override = Some("glm-4.6".into());
                } else if a.starts_with("glm") {
                    app.provider = "glm-zai".into();
                    app.model_override = Some(arg.to_string());
                } else {
                    app.model_override = Some(arg.to_string());
                }
                let m = app.model_override.clone().unwrap_or_default();
                app.status = format!("model > {m} · {}", app.provider);
                app.chat.push(Role::Agent, format!("model set to {m} · {} for new runs", app.provider));
            }
        }
        "/effort" | "/e" => {
            let e = arg.to_lowercase();
            if matches!(e.as_str(), "low" | "medium" | "high") {
                app.effort = e.clone();
                app.status = format!("effort > {e}");
            } else {
                app.chat.push(Role::Agent, format!("effort: {} — /effort low|medium|high", app.effort));
            }
        }
        "/cp" => {
            if !arg.is_empty() {
                // Adopt the project's oracle: load its .mf passport, hang its
                // tasks in the right sidebar (auto-shown), remember the project
                // so new oracle runs carry its context.
                app.passport.load(arg);
                app.project = Some(arg.to_string());
                app.right_collapsed = false;
                app.status =
                    format!("adopted '{arg}' oracle — tasks in the right panel · /ps for the plan");
            } else {
                app.chat.push(Role::Agent, "usage: /cp <project>".into());
            }
        }
        "/compact" => {
            compact_chat(app);
            app.status = "chat compacted (/compact)".into();
        }
        "/screenshot" | "/shot" => take_screenshot(app),
        other => app.chat.push(Role::Agent, format!("unknown command {other} — try /help")),
    }
    app.save_session();
    true
}

/// If `path` is an image file, read it and return `(base64, mime)`. `None` for
/// non-images / unreadable files — the caller then treats the drop as a path.
fn read_image_file(path: &std::path::Path) -> Option<(String, String)> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    };
    let bytes = std::fs::read(path).ok()?;
    // Cap at ~8 MB — z.ai vision limits are well under that; avoid a huge b64.
    if bytes.len() > 8 * 1024 * 1024 {
        return None;
    }
    Some((base64_encode(&bytes), mime.to_string()))
}

/// Standard base64 (RFC 4648) with padding — a small dependency-free encoder for
/// data: URIs. (The crate carries no base64 dep; this keeps it that way.)
fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Capture the screen to a temp PNG (gnome-screenshot), decode it into the
/// ImagePane, and show it full-screen (CenterMode::Image). Best-effort — a
/// missing tool / capture failure becomes a status line, never a panic.
fn take_screenshot(app: &mut App) {
    let tmp = std::env::temp_dir().join(format!("keiseikode-shot-{}.png", app.chat.msgs.len()));
    let ok = std::process::Command::new("gnome-screenshot")
        .arg("-f")
        .arg(&tmp)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        app.status = "screenshot failed (gnome-screenshot unavailable?)".into();
        return;
    }
    match std::fs::read(&tmp) {
        Ok(bytes) => match app.images.load(&bytes) {
            Some(id) => {
                app.center = CenterMode::Image(id);
                app.focus = Pane::Terminal;
                app.status = "screenshot — Esc back to chat".into();
            }
            None => app.status = "screenshot: could not decode the image".into(),
        },
        Err(e) => app.status = format!("screenshot: read failed: {e}"),
    }
    let _ = std::fs::remove_file(&tmp);
}

/// Open the project plan / passport full-screen on the main center (`/ps`,
/// `/plan`, Ctrl-P, or a task click). Loads the passport first if a project is
/// active; otherwise hints how to pick one.
fn show_plan(app: &mut App) {
    match app.project.clone() {
        Some(p) => {
            app.passport.load(&p);
            app.center = CenterMode::Plan;
            app.focus = Pane::Terminal;
            app.status = format!("plan · {p} — Esc > chat");
        }
        None => {
            app.chat.push(Role::Agent, "no project yet — /cp <project> first".into());
            app.status = "no project — /cp <project>".into();
        }
    }
}

/// Replace the transcript with ONE condensed "summary so far" message (first
/// user goal + last assistant state) so a long history stays usable.
fn compact_chat(app: &mut App) {
    let first_user = app
        .chat
        .msgs
        .iter()
        .find(|m| m.role == Role::User && !m.text.trim().is_empty())
        .map(|m| m.text.clone());
    let last_agent = app
        .chat
        .msgs
        .iter()
        .rev()
        .find(|m| m.role == Role::Agent && !m.text.trim().is_empty())
        .map(|m| m.text.clone());
    let n = app.chat.msgs.len();
    let mut s = format!("[compacted {n} messages]\n");
    if let Some(g) = first_user {
        s.push_str(&format!("goal: {}\n", g.chars().take(200).collect::<String>()));
    }
    if let Some(a) = last_agent {
        s.push_str(&format!("last: {}", a.chars().take(400).collect::<String>()));
    }
    app.chat.msgs.clear();
    app.chat.push(Role::Agent, s);
}

/// PLAN mode: append a `.mf` task node to the current project's passport under
/// `~/.claude/projects-state/<project>/tasks/`. Returns the written path.
fn write_plan_node(app: &App, line: &str) -> std::io::Result<String> {
    let project = app.passport.project.clone().unwrap_or_else(|| "keiseikode".to_string());
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = std::path::PathBuf::from(&home)
        .join(".claude/projects-state")
        .join(&project)
        .join("tasks");
    std::fs::create_dir_all(&dir)?;
    let slug: String = line
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(40)
        .collect();
    let slug = if slug.is_empty() { "plan".to_string() } else { slug };
    let file = dir.join(format!("plan-{slug}.mf"));
    let node = format!(
        "-- kubik: plan-{slug}\n-- does: {line}\n-- kind: task\n-- status: open\n-- project: {project}\n\n(plan drafted from KeiSeiKode chat, plan mode)\n"
    );
    std::fs::write(&file, node)?;
    Ok(file.display().to_string())
}

/// Open the tree's selected file in the editor ABOVE the chat (t21). A directory
/// just toggles. The center stays `Chat` — the editor is conditional on
/// `editor.is_open()`.
fn open_selected_in_editor(app: &mut App) {
    if app.tree.selected_is_dir() {
        app.tree.toggle_selected();
        return;
    }
    if let Some(path) = app.tree.selected_path() {
        app.editor.open(path.clone());
        app.center = CenterMode::Chat;
        app.focus = Pane::Terminal;
        app.status = format!("editing {} · Esc to close", path.display());
    }
}

fn handle_mouse(app: &mut App, m: MouseEvent, area: Rect, tx: &UnboundedSender<RunEvent>) {
    let [_header, tree_r, term_r, agents_r, modebar_r, status_r] = regions(area, app.tree_collapsed, app.right_collapsed);
    let tree_body = tree_r; // the tree fills the whole left column now (no settings tab)
    // The agent cards live in the TOP half of the right column (right_split);
    // the bottom half is the passport. Hit-test cards against the top half only.
    let [agents_r, _passport_r] = crate::ui::right_split(agents_r);
    match m.kind {
        // Status-bar buttons (bottom row): "fs" toggles the tree, "agents" opens
        // the center dashboard.
        MouseEventKind::Down(MouseButton::Left)
            if crate::ui::status_hit(status_r, m.column, m.row).is_some() =>
        {
            match crate::ui::status_hit(status_r, m.column, m.row).unwrap() {
                crate::ui::StatusHit::Mic => {
                    // Push-to-hold: start on press, the mouse-up handler stops it.
                    if app.recorder.is_none() {
                        toggle_record(app, tx);
                    }
                    app.mic_held = true;
                }
                crate::ui::StatusHit::Speaker => {
                    app.voice_reply = !app.voice_reply;
                    app.status = format!("speak replies: {}", if app.voice_reply { "on" } else { "off" });
                }
                crate::ui::StatusHit::Fs => {
                    app.tree_collapsed = !app.tree_collapsed;
                    app.left_view = crate::app::LeftView::Files;
                    app.focus = if app.tree_collapsed { Pane::Terminal } else { Pane::Tree };
                }
                crate::ui::StatusHit::Agents => {
                    app.center = match app.center {
                        CenterMode::Agents => CenterMode::Chat,
                        _ => CenterMode::Agents,
                    };
                    app.focus = Pane::Terminal;
                }
            }
        }
        MouseEventKind::Down(MouseButton::Left)
            if crate::ui::mode_bar_hit(modebar_r, m.column, m.row).is_some() =>
        {
            app.touch_bar();
            match crate::ui::mode_bar_hit(modebar_r, m.column, m.row).unwrap() {
                crate::ui::ModeBarHit::Approval => {
                    app.approval_auto = !app.approval_auto;
                    app.status = format!("approval: {}", if app.approval_auto { "auto" } else { "accept-edits" });
                }
                crate::ui::ModeBarHit::Plan => {
                    app.plan_mode = !app.plan_mode;
                    app.status = format!("plan mode: {}", if app.plan_mode { "on" } else { "off" });
                }
            }
        }
        MouseEventKind::Down(MouseButton::Left) => match pane_at(area, m.column, m.row, app.tree_collapsed, app.right_collapsed) {
            Some(Pane::Tree) => {
                // Collapsed strip > clicking it expands the tree back.
                if app.tree_collapsed {
                    app.tree_collapsed = false;
                    app.status = "tree expanded".into();
                } else {
                    app.focus = Pane::Tree;
                    // Row index inside the bordered list (top border = 1 row).
                    let idx = m.row.saturating_sub(tree_body.y + 1) as usize;
                    app.tree.select_index(idx);
                    app.dragging_from_tree = true;
                }
            }
            Some(Pane::Terminal) => {
                // Clicking the center focuses it (typing > chat). The chat is
                // primary, so a click never collapses it. When a file is open,
                // a click in the editor's top half focuses EDITING; a click in
                // the chat's lower half returns to the chat.
                app.focus = Pane::Terminal;
                if app.editor.is_open() && matches!(app.center, CenterMode::Chat) {
                    let split = term_r.y + term_r.height.saturating_mul(55) / 100;
                    app.editor_focus = m.row < split;
                }
            }
            Some(Pane::Agents) => {
                app.focus = Pane::Agents;
                if app.agents.is_expanded() {
                    // Clicking the in-sidebar detail > open it in the big chat.
                    if let Some(id) = app.agents.expanded_id() {
                        app.agents.collapse();
                        app.center = CenterMode::Agent(id);
                        app.focus = Pane::Terminal;
                    }
                } else {
                    let rel = m.row.saturating_sub(agents_r.y + 1);
                    let card_idx = (rel / CARD_ROWS) as usize;
                    if card_idx < app.agents.cards.len() {
                        // Stage 1: expand the clicked card IN the sidebar so you
                        // can watch what it's doing right there; a second click
                        // (handled above) opens it in the big window.
                        app.agents.sel = card_idx;
                        app.agents.expand_selected();
                        app.status =
                            "agent expanded in sidebar — click again to open full-screen".into();
                    } else {
                        // t13: clicking the EMPTY space of the agents panel
                        // launches a new agent (the user asked for "everything
                        // clickable, empty space = launch"). A click ON a card
                        // (handled above) inspects it — only genuinely-empty space
                        // spawns, so it is not a stray-click hazard.
                        launch_agent(app, tx);
                        app.status = "launched a new agent (click empty space) · F5 also launches".into();
                    }
                }
            }
            None => {}
        },
        MouseEventKind::ScrollUp => {
            match pane_at(area, m.column, m.row, app.tree_collapsed, app.right_collapsed) {
                Some(Pane::Tree) => app.tree.scroll(false, 3),
                _ => app.chat.scroll_up(3),
            }
        }
        MouseEventKind::ScrollDown => {
            match pane_at(area, m.column, m.row, app.tree_collapsed, app.right_collapsed) {
                Some(Pane::Tree) => app.tree.scroll(true, 3),
                _ => app.chat.scroll_down(3),
            }
        }
        MouseEventKind::Up(MouseButton::Left) if app.mic_held => {
            // Release push-to-hold mic > stop + transcribe.
            if app.recorder.is_some() {
                toggle_record(app, tx);
            }
            app.mic_held = false;
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if app.dragging_from_tree {
                match pane_at(area, m.column, m.row, app.tree_collapsed, app.right_collapsed) {
                    Some(Pane::Terminal) => {
                        if let Some(path) = app.tree.selected_path() {
                            let p = format!("{} ", path.display());
                            if matches!(app.center, CenterMode::Terminal) {
                                // Center IS the shell → drop the path into the PTY.
                                app.term.feed_str(&p);
                                app.status = format!("dropped {} into terminal", path.display());
                            } else if let Some((b64, mime)) = read_image_file(&path) {
                                // An IMAGE file dropped into the chat → attach it
                                // to the next message (glm-4.6v vision), not the
                                // path text.
                                app.pending_image = Some((b64, mime));
                                app.status = format!("📎 image attached: {} — type + Enter to ask", path.display());
                            } else {
                                // Center is the chat (the default) → drop the path
                                // into the chat input instead of the hidden PTY.
                                app.chat.input.push_str(&p);
                                app.status = format!("dropped {} into chat", path.display());
                            }
                            app.focus = Pane::Terminal;
                        }
                    }
                    Some(Pane::Tree) => activate_tree(app), // click-in-place = open/expand
                    _ => {}
                }
                app.dragging_from_tree = false;
            }
        }
        _ => {}
    }
    let _ = (term_r, tx); // tx kept for symmetry with key-driven launch paths
}

/// Activate the selected tree row: a directory toggles expand/collapse; a file
/// opens in the editor ABOVE the chat (center stays Chat) (t21).
fn activate_tree(app: &mut App) {
    if app.tree.selected_is_dir() {
        app.tree.toggle_selected();
    } else if let Some(path) = app.tree.selected_path() {
        app.editor.open(path.clone());
        app.center = CenterMode::Chat;
        app.focus = Pane::Terminal;
        app.status = format!("editing {} · Esc to close", path.display());
    }
}

/// Push the active theme's colors to the host terminal (OSC 10/11), or reset to
/// the user's own colors for the "terminal default" theme. Panes stay transparent.
fn apply_theme() {
    let _ = write!(std::io::stdout(), "{}", crate::theme::palette().osc());
    let _ = std::io::stdout().flush();
}

/// Push-to-talk toggle. First press starts the mic; second press stops it,
/// ships the captured WAV to the cortex STT endpoint on a background task, and
/// forwards the transcript as `RunEvent::Voice` (> chat input). Best-effort: a
/// missing mic / network error becomes a status line, never a panic.
fn toggle_record(app: &mut App, tx: &UnboundedSender<RunEvent>) {
    if let Some(rec) = app.recorder.take() {
        let wav = rec.stop();
        if wav.is_empty() {
            app.status = "voice: no audio captured (mic unavailable?)".into();
            return;
        }
        let cfg = RunConfig::from_env();
        let tx2 = tx.clone();
        tokio::spawn(async move {
            match crate::voice::transcribe(&cfg.base, &cfg.token, wav).await {
                Ok(text) => {
                    let _ = tx2.send(RunEvent::Voice(text));
                }
                Err(e) => {
                    let _ = tx2.send(RunEvent::Error { id: "voice".into(), msg: format!("stt: {e}") });
                }
            }
        });
        app.status = "voice: transcribing…".into();
    } else {
        match crate::voice::record_start() {
            Ok(rec) => {
                app.recorder = Some(rec);
                app.status = "voice: recording — Ctrl-Space/F2 to stop".into();
            }
            Err(e) => app.status = format!("voice: mic unavailable ({e})"),
        }
    }
}

/// Launch a GLM agent through our kei-cortex runtime (Path A) and open its live detail.
fn launch_agent(app: &mut App, tx: &UnboundedSender<RunEvent>) {
    let cfg = RunConfig::from_env();
    let label = app.next_agent_label();
    spawn_run(
        cfg,
        DEMO_PROMPT.to_string(),
        label,
        "generalist".to_string(),
        "list files in the current directory + count them".to_string(),
        tx.clone(),
    );
    app.focus = Pane::Agents;
    app.status = "launched GLM agent (our runtime, 0 Claude) — click the card for live detail".into();
}

#[cfg(test)]
mod esc_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn base64_encode_matches_known_vectors() {
        // RFC 4648 test vectors.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn read_image_file_detects_by_extension_and_skips_non_images() {
        let dir = std::env::temp_dir();
        let png = dir.join("kei-img-test.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\nfake").unwrap();
        let got = read_image_file(&png);
        assert!(got.is_some());
        assert_eq!(got.unwrap().1, "image/png");
        let txt = dir.join("kei-img-test.txt");
        std::fs::write(&txt, b"hi").unwrap();
        assert!(read_image_file(&txt).is_none(), "non-image extension → None");
    }

    fn app() -> App {
        App::new(std::env::temp_dir()).expect("init app")
    }

    fn esc(app: &mut App) {
        let (tx, _rx) = unbounded_channel::<RunEvent>();
        handle_key(app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &tx);
    }

    #[test]
    fn esc_discards_a_half_typed_line_before_touching_any_pane() {
        let mut a = app();
        a.tree_collapsed = false; // a pane Esc would otherwise close
        a.chat.input = "rm -rf everything".into();
        esc(&mut a);
        assert!(a.chat.input.is_empty(), "Esc clears the pending input");
        assert!(!a.tree_collapsed, "and consumes the keypress — the tree stays open");
    }

    #[test]
    fn esc_on_an_empty_line_falls_through_to_closing_the_pane() {
        let mut a = app();
        a.tree_collapsed = false;
        a.chat.input.clear();
        esc(&mut a);
        assert!(a.tree_collapsed, "nothing typed → Esc closes the file manager");
    }

    #[test]
    fn esc_steps_back_off_the_mode_bar() {
        let mut a = app();
        a.bar_focus = Some(1);
        esc(&mut a);
        assert_eq!(a.bar_focus, None, "Esc returns the caret from the bar to the chat");
    }

    fn press(app: &mut App, code: KeyCode) {
        let (tx, _rx) = unbounded_channel::<RunEvent>();
        handle_key(app, KeyEvent::new(code, KeyModifiers::NONE), &tx);
    }

    #[test]
    fn slash_on_empty_input_opens_the_command_palette() {
        let mut a = app();
        a.focus = Pane::Terminal; // chat has the keyboard
        assert!(!a.palette.open);
        press(&mut a, KeyCode::Char('/'));
        assert!(a.palette.open, "typing / on an empty line opens the palette");
        assert_eq!(a.chat.input, "/", "the slash is still shown in the input");
    }

    #[test]
    fn typing_in_the_palette_filters_and_arrows_move_the_selection() {
        let mut a = app();
        a.focus = Pane::Terminal;
        press(&mut a, KeyCode::Char('/'));
        for c in "model".chars() {
            press(&mut a, KeyCode::Char(c));
        }
        assert!(a.palette.open);
        assert_eq!(a.palette.current().map(|c| c.name), Some("model"));
        // Down then up returns to the top (single-match set here).
        press(&mut a, KeyCode::Down);
        press(&mut a, KeyCode::Up);
        assert_eq!(a.palette.selected, 0);
    }

    #[test]
    fn esc_closes_the_palette_and_clears_the_slash() {
        let mut a = app();
        a.focus = Pane::Terminal;
        press(&mut a, KeyCode::Char('/'));
        press(&mut a, KeyCode::Esc);
        assert!(!a.palette.open, "Esc closes the palette");
        assert!(a.chat.input.is_empty(), "and clears the leading slash");
    }

    #[test]
    fn enter_on_model_drills_into_the_models_list() {
        let mut a = app();
        a.focus = Pane::Terminal;
        press(&mut a, KeyCode::Char('/'));
        for c in "model".chars() {
            press(&mut a, KeyCode::Char(c));
        }
        press(&mut a, KeyCode::Enter);
        // /model DRILLS into the models sub-list.
        assert!(a.palette.open, "palette stays open");
        assert!(a.palette.in_drill(), "we drilled into a sub-level");
        // Selecting a model sets the override + provider AND KEEPS the window
        // open (settings-style — click through options, Esc closes).
        press(&mut a, KeyCode::Enter); // pick the first model (glm-4.6)
        assert!(a.palette.open, "picking a model keeps the settings window open");
        assert!(a.palette.in_drill());
        assert_eq!(a.model_override.as_deref(), Some("glm-4.6"));
        assert_eq!(a.provider, "glm-zai");
        // The active row now carries the ✓ (rebuilt in place).
        assert!(a.palette.current_row_action().is_some());
        // Esc closes the settings window.
        press(&mut a, KeyCode::Esc);
        assert!(!a.palette.in_drill(), "Esc pops the settings level");
    }

    #[test]
    fn effort_is_a_settings_window_that_stays_open_on_pick() {
        let mut a = app();
        a.focus = Pane::Terminal;
        press(&mut a, KeyCode::Char('/'));
        for c in "effort".chars() {
            press(&mut a, KeyCode::Char(c));
        }
        press(&mut a, KeyCode::Enter);
        assert!(a.palette.in_drill(), "/effort drills into a settings window");
        // Pick the first row (low) — stays open, effort applied.
        press(&mut a, KeyCode::Enter);
        assert!(a.palette.open, "effort window stays open on pick");
        assert_eq!(a.effort, "low");
    }

    #[test]
    fn voice_window_toggles_speak_and_stays_open() {
        let mut a = app();
        a.focus = Pane::Terminal;
        press(&mut a, KeyCode::Char('/'));
        for c in "speak".chars() {
            press(&mut a, KeyCode::Char(c));
        }
        press(&mut a, KeyCode::Enter); // drill into voice
        assert!(a.palette.in_drill());
        // Move to the "speak replies" row (index 1) and toggle it.
        let before = a.voice_reply;
        press(&mut a, KeyCode::Down);
        press(&mut a, KeyCode::Enter);
        assert_eq!(a.voice_reply, !before, "speak toggled");
        assert!(a.palette.open, "voice window stays open");
    }

    #[test]
    fn drill_into_sessions_then_esc_pops_back_to_root() {
        let mut a = app();
        a.focus = Pane::Terminal;
        press(&mut a, KeyCode::Char('/'));
        for c in "sessions".chars() {
            press(&mut a, KeyCode::Char(c));
        }
        press(&mut a, KeyCode::Enter);
        assert!(a.palette.in_drill(), "drilled into sessions");
        press(&mut a, KeyCode::Esc);
        // Esc in a drill pops ONE level back to the root (palette still open).
        assert!(a.palette.open, "still open at root");
        assert!(!a.palette.in_drill(), "popped back out of the drill");
        press(&mut a, KeyCode::Esc);
        assert!(!a.palette.open, "second Esc closes the palette");
    }

    #[test]
    fn p_pins_the_passport_into_the_sidebar_then_unpins() {
        let mut a = app();
        a.center = CenterMode::Plan; // the passport window is up
        assert!(!a.pin_passport);
        press(&mut a, KeyCode::Char('p'));
        assert!(a.pin_passport, "P pins the passport");
        assert!(!a.right_collapsed, "the sidebar opens to hold it");
        assert!(matches!(a.center, CenterMode::Chat), "and returns to the chat");
        // Re-open the passport and unpin.
        a.center = CenterMode::Plan;
        press(&mut a, KeyCode::Char('p'));
        assert!(!a.pin_passport, "P again unpins");
    }

    #[test]
    fn p_pins_the_agents_dashboard() {
        let mut a = app();
        a.center = CenterMode::Agents;
        press(&mut a, KeyCode::Char('p'));
        assert!(a.pin_agents, "P pins the agents dashboard");
        assert!(matches!(a.center, CenterMode::Chat));
    }

    #[test]
    fn p_types_normally_in_the_chat() {
        let mut a = app();
        a.focus = Pane::Terminal; // chat has the keyboard, center is Chat
        press(&mut a, KeyCode::Char('p'));
        assert_eq!(a.chat.input, "p", "P is a normal character in the chat");
        assert!(!a.pin_passport);
    }

    #[test]
    fn up_arrow_recalls_the_last_command_then_opens_history() {
        use crate::chat::Role;
        let mut a = app();
        a.focus = Pane::Terminal;
        a.chat.push(Role::User, "/model glm".into());
        a.chat.push(Role::User, "hello world".into());
        // Up#1: newest user message into the input.
        press(&mut a, KeyCode::Up);
        assert_eq!(a.chat.input, "hello world", "Up#1 recalls the last command");
        assert!(!a.palette.open);
        // Up#2: opens the history window (palette drill).
        press(&mut a, KeyCode::Up);
        assert!(a.palette.open && a.palette.in_drill(), "Up#2 opens the history list");
    }

    #[test]
    fn drill_into_agentslib_opens_dna_on_pick() {
        let mut a = app();
        a.focus = Pane::Terminal;
        press(&mut a, KeyCode::Char('/'));
        for c in "agentslib".chars() {
            press(&mut a, KeyCode::Char(c));
        }
        press(&mut a, KeyCode::Enter);
        assert!(a.palette.in_drill(), "drilled into the agent library");
        // Pick the first agent (if the roster is non-empty on this host).
        if a.palette.current_row_action().is_some() {
            press(&mut a, KeyCode::Enter);
            assert!(matches!(a.center, CenterMode::Dna), "picking an agent opens its DNA");
            assert!(!a.palette.open);
        }
    }

    #[test]
    fn enter_on_a_run_command_executes_and_clears() {
        let mut a = app();
        a.focus = Pane::Terminal;
        a.tree_collapsed = true;
        press(&mut a, KeyCode::Char('/'));
        for c in "files".chars() {
            press(&mut a, KeyCode::Char(c));
        }
        press(&mut a, KeyCode::Enter);
        assert!(!a.palette.open);
        assert!(a.chat.input.is_empty(), "a Run command clears the input");
        assert!(!a.tree_collapsed, "/files toggled the file manager open");
    }
}
