//! kei-tui `agents` pane — the right-sidebar stack of agent-run "mini-window" cards.
//!
//! PURE RENDER + SELECTION. No async, no HTTP, no I/O — the orchestrator owns the
//! SSE client and feeds `AgentCard` rows into `AgentsPane::upsert`. This module
//! only knows how to draw the cards and move a selection cursor over them.

use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

/// Lifecycle status of a single agent run. Drives the colored status dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Running,
    Done,
    Error,
}

impl AgentStatus {
    /// Color of the status dot per the pane spec:
    /// Running=Yellow, Done=Green, Error=Red, Idle=Gray.
    pub fn dot_color(self) -> Color {
        match self {
            AgentStatus::Running => crate::theme::palette().accent2,
            AgentStatus::Done => crate::theme::palette().done,
            AgentStatus::Error => crate::theme::palette().accent,
            AgentStatus::Idle => crate::theme::palette().muted,
        }
    }
}

/// One agent run, rendered as a single bordered mini-window (+ a detail view).
#[derive(Debug, Clone)]
pub struct AgentCard {
    pub id: String,
    pub label: String,
    pub role: String,
    pub task: String,
    pub status: AgentStatus,
    pub last_tool: Option<String>,
    pub tokens: u32,
    pub started: std::time::Instant,
    /// Rolling live log (tool calls + streamed text) shown in the detail view.
    pub log: Vec<String>,
}

/// The right-sidebar agents pane: a vertical stack of mini-window cards plus a
/// selection cursor. Empty by default.
#[derive(Debug, Default)]
pub struct AgentsPane {
    pub cards: Vec<AgentCard>,
    pub sel: usize,
    /// When `Some(i)`, card `i` is expanded IN PLACE in the sidebar (its live
    /// detail fills the pane) — the first stage before optionally opening it in
    /// the big center chat. `None` = the stacked card list.
    pub expanded: Option<usize>,
}

impl AgentsPane {
    /// An empty pane (no cards, selection at 0).
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a new card, or — if a card with the same `id` already exists —
    /// update it in place (keeps the card count stable, preserves order).
    pub fn upsert(&mut self, card: AgentCard) {
        if let Some(slot) = self.cards.iter_mut().find(|c| c.id == card.id) {
            // Preserve accumulated runtime state (started/log/tokens/last_tool);
            // refresh only identity + status.
            slot.label = card.label;
            slot.role = card.role;
            slot.task = card.task;
            slot.status = card.status;
        } else {
            self.cards.push(card);
        }
    }

    /// Up/Down: move the selection cursor over the cards. Clamps at both ends
    /// and is a no-op when there are no cards.
    pub fn on_key(&mut self, code: KeyCode) {
        if self.cards.is_empty() {
            return;
        }
        match code {
            KeyCode::Up => {
                self.sel = self.sel.saturating_sub(1);
            }
            KeyCode::Down => {
                let last = self.cards.len() - 1;
                if self.sel < last {
                    self.sel += 1;
                }
            }
            _ => {}
        }
    }

    /// The `id` of the currently-selected card, or `None` when the pane is empty.
    pub fn selected_id(&self) -> Option<String> {
        self.cards.get(self.sel).map(|c| c.id.clone())
    }

    /// Expand the selected card IN PLACE (stage 1 — view it in the sidebar).
    pub fn expand_selected(&mut self) {
        if self.sel < self.cards.len() {
            self.expanded = Some(self.sel);
        }
    }

    /// Collapse the in-sidebar expansion back to the card list.
    pub fn collapse(&mut self) {
        self.expanded = None;
    }

    /// True while a card is expanded in the sidebar.
    pub fn is_expanded(&self) -> bool {
        self.expanded.is_some()
    }

    /// The id of the card currently expanded in the sidebar, if any.
    pub fn expanded_id(&self) -> Option<String> {
        self.expanded.and_then(|i| self.cards.get(i)).map(|c| c.id.clone())
    }

    /// Render the outer ` agents ` pane (cyan border when `focused`, dark-gray
    /// otherwise) with one bordered mini-window per card stacked vertically.
    ///
    /// Empty pane > a single dim `"no agents this session"` line.
    ///
    /// Each card:
    ///   line 1 — `label` + a colored status dot (Running=Yellow, Done=Green,
    ///            Error=Red, Idle=Gray)
    ///   line 2 — `last_tool` (or `—` when absent) + the token count
    ///
    /// The selected card's border is Cyan; every other card's border is DarkGray.
    /// Cards beyond what fits in `area` are ignored (no overflow rendering).
    pub fn render(&self, f: &mut Frame, area: Rect, focused: bool) {
        // Stage 1: a card is expanded IN the sidebar — show its live detail here
        // (Enter > open in the big chat · Esc > back to the list).
        if let Some(i) = self.expanded {
            if let Some(card) = self.cards.get(i) {
                render_detail(f, area, card);
                return;
            }
        }
        let outer_style = if focused {
            Style::default().fg(crate::theme::palette().ink)
        } else {
            Style::default().fg(crate::theme::palette().grid)
        };
        let outer = Block::default()
            .borders(Borders::ALL)
            .title(" agents ")
            .border_style(outer_style);
        let inner = outer.inner(area);
        f.render_widget(outer, area);

        if self.cards.is_empty() {
            let empty = Paragraph::new(vec![
                Line::from(Span::styled("no agents yet", Style::default().fg(crate::theme::palette().muted))),
                Line::from(Span::styled("F5 launches one; click a", Style::default().fg(crate::theme::palette().muted))),
                Line::from(Span::styled("card to watch it work", Style::default().fg(crate::theme::palette().muted))),
            ]);
            f.render_widget(empty, inner);
            return;
        }

        // Frameless cards: 2 rows each — the agent NAME on top, the TOPIC it is
        // working on below. No border (a bare line, not a bordered box).
        let n = self.cards.len();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(2); n])
            .split(inner);

        let pal = crate::theme::palette();
        for (i, card) in self.cards.iter().enumerate() {
            let card_area = match chunks.get(i) {
                Some(r) => *r,
                None => break,
            };
            if card_area.height == 0 {
                break;
            }

            // Line 1: status dot + agent NAME (label) in ink.
            let line1 = Line::from(vec![
                Span::styled("●", Style::default().fg(card.status.dot_color())),
                Span::raw(" "),
                Span::styled(card.label.clone(), Style::default().fg(pal.ink)),
            ]);

            // Line 2: the TOPIC the agent is working on (its task), dimmed —
            // truncated with an ellipsis if wider than the card.
            let w = card_area.width.saturating_sub(2) as usize;
            let topic = truncate_text(&card.task, w);
            let line2 = Line::from(vec![
                Span::raw("  "),
                Span::styled(topic, Style::default().fg(pal.muted)),
            ]);

            f.render_widget(Paragraph::new(vec![line1, line2]), card_area);
        }
    }
}

/// Render the AGENTS DASHBOARD in the center — modelled on Claude Code's agent
/// list: a header count, then the agents grouped under "Working" and
/// "Completed", each row = a status marker + label + its current task/tool +
/// elapsed. `elapsed_ms` drives the spinner marker on the working ones.
pub fn render_dashboard(f: &mut Frame, area: Rect, pane: &AgentsPane, elapsed_ms: u128) {
    let pal = crate::theme::palette();
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .title(" agents · this session ")
        .title_style(Style::default().fg(pal.ink));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let dim = Style::default().fg(pal.muted);
    let working: Vec<&AgentCard> = pane.cards.iter().filter(|c| c.status == AgentStatus::Running).collect();
    let done: Vec<&AgentCard> = pane.cards.iter().filter(|c| c.status != AgentStatus::Running).collect();

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        format!("{} working · {} completed   ·   F5 launch · Ctrl-P plan · Esc back", working.len(), done.len()),
        dim,
    )));
    lines.push(Line::from(""));

    if pane.cards.is_empty() {
        lines.push(Line::from(Span::styled("no agents this session yet.", dim)));
        lines.push(Line::from(Span::styled("the oracle launches sub-agents for you; F5 starts a demo.", dim)));
    }

    // A row: marker + label + task + elapsed (right-ish).
    let row = |marker: Span<'static>, c: &AgentCard| -> Line<'static> {
        let secs = c.started.elapsed().as_secs();
        let clock = if secs >= 60 { format!("{}m", secs / 60) } else { format!("{secs}s") };
        let doing = c.last_tool.clone().unwrap_or_else(|| c.task.clone());
        Line::from(vec![
            marker,
            Span::styled(format!(" {}  ", c.label), Style::default().fg(pal.ink)),
            Span::styled(format!("{doing}"), Style::default().fg(pal.accent2)),
            Span::styled(format!("   {} tok · {clock}", c.tokens), Style::default().fg(pal.muted)),
        ])
    };

    if !working.is_empty() {
        lines.push(Line::from(Span::styled("Working", Style::default().fg(pal.done))));
        for c in &working {
            let sph = crate::sphere::glyph(elapsed_ms).to_string();
            lines.push(row(Span::styled(format!("  {sph}"), Style::default().fg(pal.accent)), c));
            lines.push(Line::from(Span::styled(format!("     └ {}", c.task), dim)));
        }
        lines.push(Line::from(""));
    }
    if !done.is_empty() {
        lines.push(Line::from(Span::styled("Completed", Style::default().fg(pal.muted))));
        for c in &done {
            let mark = if c.status == AgentStatus::Error { "  x" } else { "  ·" };
            let col = if c.status == AgentStatus::Error { pal.accent } else { pal.done };
            lines.push(row(Span::styled(mark.to_string(), Style::default().fg(col)), c));
        }
    }

    f.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

/// Render the full detail view for one agent run — used by the cockpit center
/// pane when a card is opened. Header (role/task/status/elapsed/tokens/tool) +
/// the tail of the live log.
pub fn render_detail(f: &mut Frame, area: Rect, card: &AgentCard) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" agent · {} ", card.label))
        .border_style(Style::default().fg(crate::theme::palette().grid));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let elapsed = card.started.elapsed().as_secs();
    let status_txt = match card.status {
        AgentStatus::Running => "running",
        AgentStatus::Done => "done",
        AgentStatus::Error => "error",
        AgentStatus::Idle => "idle",
    };
    let dim = Style::default().fg(crate::theme::palette().muted);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("█ ", Style::default().fg(card.status.dot_color())),
            Span::styled(status_txt, Style::default().fg(card.status.dot_color())),
            Span::styled(format!("   {elapsed}s   {} tok", card.tokens), dim),
        ]),
        Line::from(vec![Span::styled("role:  ", dim), Span::raw(card.role.clone())]),
        Line::from(vec![Span::styled("task:  ", dim), Span::raw(card.task.clone())]),
        Line::from(vec![
            Span::styled("tool:  ", dim),
            Span::raw(card.last_tool.clone().unwrap_or_else(|| "—".into())),
        ]),
    ];
    // Spinning Frobenius sphere while the agent is thinking.
    if card.status == AgentStatus::Running {
        lines.push(Line::from(Span::styled(
            crate::sphere::line(card.started.elapsed().as_millis()),
            Style::default().fg(crate::theme::palette().accent),
        )));
    }
    lines.push(Line::from(Span::styled("── live ──────────────", dim)));
    let header = lines.len() as u16;
    let avail = inner.height.saturating_sub(header + 1).max(1) as usize;
    let start = card.log.len().saturating_sub(avail);
    for l in &card.log[start..] {
        lines.push(Line::from(l.clone()));
    }
    lines.push(Line::from(Span::styled(
        "Esc: close · click terminal to return",
        dim,
    )));
    f.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

/// Truncate `s` to fit `max` chars, appending a single-char ellipsis `…` when
/// it is longer. `max == 0` → empty string (a 0-width card shows nothing).
fn truncate_text(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    if max == 1 {
        return "…".to_string();
    }
    let head: String = chars[..max - 1].iter().collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn card(id: &str, label: &str, status: AgentStatus) -> AgentCard {
        AgentCard {
            id: id.to_string(),
            label: label.to_string(),
            role: "generalist".to_string(),
            task: "demo".to_string(),
            status,
            last_tool: Some("bash".to_string()),
            tokens: 42,
            started: std::time::Instant::now(),
            log: Vec::new(),
        }
    }

    #[test]
    fn two_cards_render_and_select() {
        let mut pane = AgentsPane::new();
        pane.upsert(card("a1", "researcher", AgentStatus::Running));
        pane.upsert(card("a2", "builder", AgentStatus::Done));

        // upsert with an existing id updates in place > len stays 2.
        pane.upsert(AgentCard {
            id: "a1".to_string(),
            label: "researcher".to_string(),
            role: "generalist".to_string(),
            task: "demo".to_string(),
            status: AgentStatus::Error,
            last_tool: Some("edit".to_string()),
            tokens: 99,
            started: std::time::Instant::now(),
            log: Vec::new(),
        });
        assert_eq!(pane.cards.len(), 2);
        assert_eq!(pane.cards[0].status, AgentStatus::Error);
        // merge upsert preserves accumulated tokens (42), refreshes status only
        assert_eq!(pane.cards[0].tokens, 42);

        // selected_id: default cursor at 0 > first card.
        assert_eq!(pane.selected_id(), Some("a1".to_string()));

        // Down > second card; Down again clamps at the last index.
        pane.on_key(KeyCode::Down);
        assert_eq!(pane.selected_id(), Some("a2".to_string()));
        pane.on_key(KeyCode::Down);
        assert_eq!(pane.selected_id(), Some("a2".to_string()));

        // Up returns to the first card.
        pane.on_key(KeyCode::Up);
        assert_eq!(pane.selected_id(), Some("a1".to_string()));

        // Render against a 30x20 TestBackend — focused and unfocused, no panic.
        let backend = TestBackend::new(30, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| pane.render(f, f.area(), true)).unwrap();
        terminal.draw(|f| pane.render(f, f.area(), false)).unwrap();
    }

    #[test]
    fn empty_pane_renders_and_clamps() {
        let mut pane = AgentsPane::new();
        assert_eq!(pane.selected_id(), None);

        // on_key is a no-op on an empty pane (must not panic / overflow sel).
        pane.on_key(KeyCode::Down);
        pane.on_key(KeyCode::Up);
        assert_eq!(pane.sel, 0);

        let backend = TestBackend::new(30, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| pane.render(f, f.area(), true)).unwrap();
    }
}
