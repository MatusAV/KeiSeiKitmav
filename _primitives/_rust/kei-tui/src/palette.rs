//! Slash-command palette — the Claude-Code / Grok style pop-up that appears
//! when you type `/` at the start of the chat input. It lists every command
//! grouped by CATEGORY (like a tree), filters as you keep typing, and you move
//! the selection with ↑/↓ (skipping the category headers). Enter runs the
//! command (or, for commands that take an argument, drops `/<name> ` into the
//! input so you can finish it); Esc closes without touching what you typed.
//!
//! The palette owns NO behaviour — `runner.rs` reads the selected `Cmd` and
//! acts on it. This module is pure state + a render, so it is unit-testable
//! headlessly.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// What pressing Enter on a command should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdKind {
    /// Run immediately (navigation / toggle) — no argument needed.
    Run,
    /// Needs an argument: Enter drops `/<name> ` into the input to finish.
    NeedsArg,
}

/// One command in the palette.
#[derive(Debug, Clone, Copy)]
pub struct Cmd {
    pub category: &'static str,
    /// Canonical name WITHOUT the leading slash (e.g. "model").
    pub name: &'static str,
    /// Extra alias names (without slash) that also match the filter.
    pub aliases: &'static [&'static str],
    pub hint: &'static str,
    pub kind: CmdKind,
}

/// The full command registry, in display order (grouped by category).
pub const COMMANDS: &[Cmd] = &[
    // NAVIGATION
    Cmd { category: "navigation", name: "files", aliases: &["f", "file"], hint: "file manager (left) · F4", kind: CmdKind::Run },
    Cmd { category: "navigation", name: "agents", aliases: &["a", "agent"], hint: "live agents (this session)", kind: CmdKind::Run },
    Cmd { category: "navigation", name: "agentslib", aliases: &["alib", "lib"], hint: "agent library · DNA", kind: CmdKind::Run },
    Cmd { category: "navigation", name: "plan", aliases: &["ps"], hint: "project passport · Ctrl-P", kind: CmdKind::Run },
    Cmd { category: "navigation", name: "terminal", aliases: &["term"], hint: "embedded shell · F4", kind: CmdKind::Run },
    Cmd { category: "navigation", name: "settings", aliases: &[], hint: "theme · provider · F8", kind: CmdKind::Run },
    // SESSION
    Cmd { category: "session", name: "new", aliases: &[], hint: "start a fresh chat session", kind: CmdKind::Run },
    Cmd { category: "session", name: "sessions", aliases: &[], hint: "list / load saved sessions", kind: CmdKind::NeedsArg },
    Cmd { category: "session", name: "compact", aliases: &[], hint: "summarise + shrink the chat", kind: CmdKind::Run },
    Cmd { category: "session", name: "cp", aliases: &[], hint: "adopt a project's oracle", kind: CmdKind::NeedsArg },
    // MODEL
    Cmd { category: "model", name: "model", aliases: &["m"], hint: "claude · glm · <name>", kind: CmdKind::NeedsArg },
    Cmd { category: "model", name: "effort", aliases: &["e"], hint: "low · medium · high", kind: CmdKind::NeedsArg },
    // VOICE
    Cmd { category: "voice", name: "mic", aliases: &["mo", "moff"], hint: "microphone on/off · F2", kind: CmdKind::Run },
    Cmd { category: "voice", name: "speak", aliases: &["so", "soff"], hint: "speak replies aloud · F7", kind: CmdKind::Run },
    // HELP
    Cmd { category: "help", name: "screenshot", aliases: &["shot"], hint: "capture the screen inline", kind: CmdKind::Run },
    Cmd { category: "help", name: "help", aliases: &[], hint: "this list", kind: CmdKind::Run },
    Cmd { category: "help", name: "tools", aliases: &[], hint: "the agent's tools", kind: CmdKind::Run },
    Cmd { category: "help", name: "skills", aliases: &[], hint: "the skill library", kind: CmdKind::Run },
];

/// What activating a dynamic row does — interpreted by `runner.rs` (the palette
/// itself has no side effects).
///
/// Two behaviours on Enter (see [`RowAction::stays_open`]):
///   * SETTINGS-style rows (SetModel / SetEffort / SetTheme / ToggleMic /
///     ToggleSpeak / SetApproval) apply the change and KEEP the window open, so
///     you can click through options; Esc closes — exactly like a settings pane.
///   * NAVIGATION rows (LoadSession / OpenAgent / OpenDna / Recall) act and close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowAction {
    /// Load a saved session by id. (closes)
    LoadSession(String),
    /// Set the model (value = model name; provider inferred by the runner). (stays)
    SetModel(String),
    /// Set reasoning effort (low|medium|high). (stays)
    SetEffort(String),
    /// Cycle to a named theme. (stays)
    SetTheme(String),
    /// Toggle the microphone on/off. (stays)
    ToggleMic,
    /// Toggle spoken replies on/off. (stays)
    ToggleSpeak,
    /// Set approval mode (value "auto" | "accept-edits"). (stays)
    SetApproval(String),
    /// Open a live agent's detail full-screen (value = run id). (closes)
    OpenAgent(String),
    /// Open a library agent's DNA view (value = agent name / manifest stem). (closes)
    OpenDna(String),
    /// Fill the input with a recalled command (value = the command text). (closes)
    Recall(String),
}

impl RowAction {
    /// True for SETTINGS-style rows: applying keeps the palette open so the user
    /// can keep clicking; only Esc closes. False for navigation rows (act+close).
    pub fn stays_open(&self) -> bool {
        matches!(
            self,
            RowAction::SetModel(_)
                | RowAction::SetEffort(_)
                | RowAction::SetTheme(_)
                | RowAction::ToggleMic
                | RowAction::ToggleSpeak
                | RowAction::SetApproval(_)
        )
    }
}

/// One row in a DYNAMIC drill level (sessions / models / agents / history).
#[derive(Debug, Clone)]
pub struct DynRow {
    pub label: String,
    pub hint: String,
    pub action: RowAction,
    /// True when this row is the CURRENTLY active choice (drawn with a ✓ / dot),
    /// so a settings-style level shows what is selected as you click through.
    pub active: bool,
}

/// A pushed drill level — a titled list the user navigates. Built by the runner
/// (which owns the data: sessions, models, agents) and pushed onto the palette
/// stack; the palette just renders + navigates it.
#[derive(Debug, Clone)]
pub struct Level {
    pub title: String,
    pub rows: Vec<DynRow>,
    pub selected: usize,
}

impl Level {
    pub fn new(title: impl Into<String>, rows: Vec<DynRow>) -> Self {
        Self { title: title.into(), rows, selected: 0 }
    }
}

/// The pop-up palette state. The ROOT level is the static command list (filtered
/// by `filter`); pushing a `Level` onto `stack` drills into a sub-list
/// (sessions / models / history …). Esc/Left pops; empty stack + close exits.
#[derive(Debug, Clone, Default)]
pub struct CommandPalette {
    pub open: bool,
    /// The typed text WITHOUT the leading slash (drives the ROOT filter).
    pub filter: String,
    /// Index into the CURRENTLY FILTERED root command list.
    pub selected: usize,
    /// Drill levels above the root (last = the one on screen). Empty = root.
    pub stack: Vec<Level>,
}

impl CommandPalette {
    /// Open with an empty filter at the ROOT command list.
    pub fn open(&mut self) {
        self.open = true;
        self.filter.clear();
        self.selected = 0;
        self.stack.clear();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.filter.clear();
        self.selected = 0;
        self.stack.clear();
    }

    /// Push a drill level (e.g. the sessions list) built by the runner.
    pub fn drill(&mut self, level: Level) {
        self.stack.push(level);
    }

    /// True when a drill level is on screen (not the root command list).
    pub fn in_drill(&self) -> bool {
        !self.stack.is_empty()
    }

    /// The action of the row under the cursor in the current DRILL level, if any.
    pub fn current_row_action(&self) -> Option<RowAction> {
        let lvl = self.stack.last()?;
        lvl.rows.get(lvl.selected).map(|r| r.action.clone())
    }

    /// Pop one drill level; returns false when already at the root (caller
    /// closes). Esc/Left steps back out of the tree one window at a time.
    pub fn pop(&mut self) -> bool {
        self.stack.pop().is_some()
    }

    /// Append a char to the filter, re-clamping the selection.
    pub fn push(&mut self, c: char) {
        self.filter.push(c);
        self.clamp();
    }

    /// Delete the last filter char; returns `false` when the filter was already
    /// empty (so the caller can close the palette on the backspace that eats the
    /// leading slash). In a drill level, Backspace pops the level instead.
    pub fn backspace(&mut self) -> bool {
        if self.in_drill() {
            return self.pop();
        }
        if self.filter.pop().is_none() {
            return false;
        }
        self.clamp();
        true
    }

    /// Commands matching the current filter (prefix on name, or substring on any
    /// alias / the name), in registry order.
    pub fn matches(&self) -> Vec<&'static Cmd> {
        let f = self.filter.to_lowercase();
        COMMANDS
            .iter()
            .filter(|c| {
                if f.is_empty() {
                    return true;
                }
                c.name.starts_with(&f)
                    || c.name.contains(&f)
                    || c.aliases.iter().any(|a| a.starts_with(&f))
                    || c.category.starts_with(&f)
            })
            .collect()
    }

    /// The command under the selection cursor, if any match.
    pub fn current(&self) -> Option<&'static Cmd> {
        self.matches().get(self.selected).copied()
    }

    pub fn move_down(&mut self) {
        if let Some(lvl) = self.stack.last_mut() {
            let n = lvl.rows.len();
            if n > 0 {
                lvl.selected = (lvl.selected + 1).min(n - 1);
            }
            return;
        }
        let n = self.matches().len();
        if n > 0 {
            self.selected = (self.selected + 1).min(n - 1);
        }
    }

    pub fn move_up(&mut self) {
        if let Some(lvl) = self.stack.last_mut() {
            lvl.selected = lvl.selected.saturating_sub(1);
            return;
        }
        self.selected = self.selected.saturating_sub(1);
    }

    fn clamp(&mut self) {
        let n = self.matches().len();
        self.selected = if n == 0 { 0 } else { self.selected.min(n - 1) };
    }

    /// Render the palette as a bordered pop-up whose BOTTOM sits just above
    /// `input_area` (it grows upward). Draws nothing when closed or when no
    /// command matches.
    pub fn render(&self, f: &mut Frame, input_area: Rect) {
        if !self.open {
            return;
        }
        let pal = crate::theme::palette();

        // In a DRILL level, render its rows + title (breadcrumb). At the ROOT,
        // render the filtered, category-grouped command list.
        let (lines, title): (Vec<Line>, String) = if let Some(lvl) = self.stack.last() {
            let mut ls: Vec<Line> = Vec::new();
            if lvl.rows.is_empty() {
                ls.push(Line::from(Span::styled("(empty)", Style::default().fg(pal.muted))));
            }
            for (i, r) in lvl.rows.iter().enumerate() {
                let selected = i == lvl.selected;
                let fg = if selected { pal.done } else { pal.ink };
                let marker = if selected { "▸ " } else { "  " };
                // The active choice (in a settings-style level) gets a ✓ dot.
                let check = if r.active { "✓ " } else { "  " };
                let check_fg = if r.active { pal.done } else { pal.muted };
                ls.push(Line::from(vec![
                    Span::styled(marker, Style::default().fg(pal.done)),
                    Span::styled(check, Style::default().fg(check_fg)),
                    Span::styled(r.label.clone(), Style::default().fg(fg)),
                    Span::styled(format!("  {}", r.hint), Style::default().fg(pal.muted)),
                ]));
            }
            (ls, format!(" {} · Esc back ", lvl.title))
        } else {
            let matches = self.matches();
            if matches.is_empty() {
                return;
            }
            let mut ls: Vec<Line> = Vec::new();
            let mut last_cat = "";
            for (i, c) in matches.iter().enumerate() {
                if c.category != last_cat {
                    last_cat = c.category;
                    ls.push(Line::from(Span::styled(
                        c.category.to_uppercase(),
                        Style::default().fg(pal.muted).add_modifier(Modifier::UNDERLINED),
                    )));
                }
                let selected = i == self.selected;
                let name_fg = if selected { pal.done } else { pal.ink };
                let marker = if selected { "▸ " } else { "  " };
                ls.push(Line::from(vec![
                    Span::styled(marker, Style::default().fg(pal.done)),
                    Span::styled(format!("/{:<10}", c.name), Style::default().fg(name_fg)),
                    Span::styled(format!(" {}", c.hint), Style::default().fg(pal.muted)),
                ]));
            }
            (ls, format!(" /{} ", self.filter))
        };

        // Height: content + top/bottom border, capped so it never eats the whole
        // screen; sits directly above the input, growing upward.
        let want = lines.len() as u16 + 2;
        let max_h = input_area.y.saturating_sub(1).max(3);
        let h = want.min(max_h).max(3);
        let y = input_area.y.saturating_sub(h);
        let area = Rect { x: input_area.x, y, width: input_area.width, height: h };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(pal.grid))
            .title(title);
        let inner = block.inner(area);
        f.render_widget(Clear, area);
        f.render_widget(block, area);
        f.render_widget(Paragraph::new(lines), inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_empty_shows_all_commands() {
        let mut p = CommandPalette::default();
        p.open();
        assert!(p.open);
        assert_eq!(p.matches().len(), COMMANDS.len());
    }

    #[test]
    fn filter_narrows_by_name_prefix() {
        let mut p = CommandPalette::default();
        p.open();
        for c in "mod".chars() {
            p.push(c);
        }
        let m = p.matches();
        assert!(m.iter().any(|c| c.name == "model"), "model matches 'mod'");
        // Every match justifies itself by name/alias/category (the same OR the
        // filter uses) — no unrelated command sneaks in.
        assert!(m.iter().all(|c| {
            let f = "mod";
            c.name.starts_with(f) || c.name.contains(f)
                || c.aliases.iter().any(|a| a.starts_with(f))
                || c.category.starts_with(f)
        }));
    }

    #[test]
    fn filter_matches_category() {
        let mut p = CommandPalette::default();
        p.open();
        for c in "voice".chars() {
            p.push(c);
        }
        let m = p.matches();
        assert!(!m.is_empty());
        assert!(m.iter().all(|c| c.category == "voice"));
    }

    #[test]
    fn move_down_clamps_at_the_end_and_up_at_the_start() {
        let mut p = CommandPalette::default();
        p.open();
        // Filter to a small set.
        for c in "model".chars() {
            p.push(c);
        }
        let n = p.matches().len();
        for _ in 0..(n + 5) {
            p.move_down();
        }
        assert_eq!(p.selected, n - 1, "never past the last match");
        for _ in 0..(n + 5) {
            p.move_up();
        }
        assert_eq!(p.selected, 0, "never before the first match");
    }

    #[test]
    fn backspace_on_empty_filter_signals_close() {
        let mut p = CommandPalette::default();
        p.open();
        assert!(!p.backspace(), "empty filter → false (caller closes)");
        p.push('m');
        assert!(p.backspace(), "non-empty → true (just deletes)");
    }

    #[test]
    fn current_returns_the_selected_command() {
        let mut p = CommandPalette::default();
        p.open();
        for c in "model".chars() {
            p.push(c);
        }
        assert_eq!(p.current().map(|c| c.name), Some("model"));
    }

    #[test]
    fn render_shows_category_headers_and_commands_on_screen() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut p = CommandPalette::default();
        p.open();
        let backend = TestBackend::new(60, 24);
        let mut term = Terminal::new(backend).unwrap();
        let input = Rect { x: 0, y: 22, width: 60, height: 1 };
        term.draw(|f| p.render(f, input)).unwrap();
        let screen: String = term.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        // Category headers (tree-like grouping) are drawn…
        assert!(screen.contains("NAVIGATION"), "NAVIGATION header rendered");
        assert!(screen.contains("MODEL"), "MODEL header rendered");
        // …and the commands under them.
        assert!(screen.contains("/model"), "/model command rendered");
        assert!(screen.contains("/files"), "/files command rendered");
        // The selection marker sits on the first row.
        assert!(screen.contains('▸'), "the selection marker is drawn");
    }
}
