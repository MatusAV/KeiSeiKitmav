//! Chat pane — a simple scrolling message history + a 3-row input box.
//!
//! Pure render + local state: no async, no I/O. The orchestrator drives
//! `ChatPane` by feeding key events into `on_char`/`backspace`, pulling a
//! submitted line via `take_input`, and appending agent replies (including
//! streaming token-by-token via `msgs.last_mut()`) directly into `msgs`.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use serde::{Deserialize, Serialize};

use crate::theme::palette;

/// Who authored a chat message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    User,
    Agent,
    /// A tool ACTION line (e.g. "> edit main.rs  +12 −3") streamed into the chat
    /// like Claude Code's process log. Rendered dim; EXCLUDED from model replay.
    Tool,
}

impl Role {
    /// The OpenAI/Anthropic chat role this maps to when we replay the whole
    /// transcript to `/v1/runs` for memory. Tool lines are filtered out before
    /// replay, so this is a harmless default for them.
    pub fn wire(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Agent | Role::Tool => "assistant",
        }
    }
}

/// A single chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Msg {
    pub role: Role,
    pub text: String,
    /// An inline-image id (into `ImagePane`'s cache) attached to this message,
    /// e.g. a screenshot. `#[serde(default)]` so sessions saved before images
    /// existed still deserialize. Not persisted meaningfully across restart (the
    /// ImagePane cache is in-memory) — a reloaded session shows the text only.
    #[serde(default, skip_serializing)]
    pub image: Option<usize>,
}

/// State for the chat pane: full message history + the in-progress input line.
#[derive(Debug, Clone)]
pub struct ChatPane {
    pub msgs: Vec<Msg>,
    pub input: String,
    /// Lines scrolled UP from the bottom. 0 = follow the newest (tail). Wheel /
    /// PageUp increases it (older); PageDown decreases it.
    pub scroll: u16,
    /// A one-line banner shown as the FIRST row of the scrollable history
    /// (KEISEIKODE · model · cwd). It is NOT a sticky top bar: it appears at the
    /// start and rides up off-screen as the chat fills — "the header turns up at
    /// first, then leaves with the chat".
    pub banner: Option<String>,
}

impl Default for ChatPane {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatPane {
    /// A fresh, empty chat pane.
    pub fn new() -> Self {
        Self {
            msgs: Vec::new(),
            input: String::new(),
            scroll: 0,
            banner: None,
        }
    }

    /// Scroll the history up (older) / down (newer). `scroll_down` toward 0
    /// re-follows the tail.
    pub fn scroll_up(&mut self, n: u16) {
        self.scroll = self.scroll.saturating_add(n);
    }
    pub fn scroll_down(&mut self, n: u16) {
        self.scroll = self.scroll.saturating_sub(n);
    }

    /// Append a new message to the history.
    pub fn push(&mut self, role: Role, text: String) {
        self.msgs.push(Msg { role, text, image: None });
    }

    /// Append a message that carries an inline image (id into ImagePane cache).
    pub fn push_image(&mut self, role: Role, text: String, image_id: usize) {
        self.msgs.push(Msg { role, text, image: Some(image_id) });
    }

    /// Hard-wrap each line to `width` columns, preserving every span's style
    /// and the line's alignment. Returns one `Line` per DISPLAYED row, so the
    /// caller's `Vec.len()` equals the true rendered height — making tail-scroll
    /// exact (see the scroll block that calls this). `width == 0` → pass-through.
    fn hard_wrap_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
        if width == 0 {
            return lines;
        }
        let mut out: Vec<Line<'static>> = Vec::with_capacity(lines.len());
        for line in lines {
            let alignment = line.alignment.unwrap_or(Alignment::Left);
            let mut cur: Vec<Span<'static>> = Vec::new();
            let mut col = 0usize;
            for span in line.spans {
                let style = span.style;
                for ch in span.content.chars() {
                    if col >= width {
                        let row = Line::from(std::mem::take(&mut cur)).alignment(alignment);
                        out.push(row);
                        col = 0;
                    }
                    cur.push(Span::styled(ch.to_string(), style));
                    col += 1;
                }
            }
            // Flush the trailing row (always at least one, even for an empty line).
            let row = Line::from(cur).alignment(alignment);
            out.push(row);
        }
        out
    }

    /// Append one character to the pending input line.
    pub fn on_char(&mut self, c: char) {
        self.input.push(c);
    }

    /// Remove the last character of the pending input line, if any.
    pub fn backspace(&mut self) {
        self.input.pop();
    }

    /// Discard whatever is typed but not sent. `true` when there was something
    /// to discard — Esc consumes the keypress on a non-empty line, and only
    /// falls through to closing panes when the line is already empty.
    pub fn clear_input(&mut self) -> bool {
        if self.input.is_empty() {
            false
        } else {
            self.input.clear();
            true
        }
    }

    /// Take the current input (clearing it) if it holds anything but
    /// whitespace. The orchestrator calls this on Enter and, if `Some`,
    /// forwards the text to the agent.
    pub fn take_input(&mut self) -> Option<String> {
        if self.input.trim().is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.input))
        }
    }

    /// Render the chat pane: bordered box titled " chat ", message history on
    /// top, a spinning-Frobenius-sphere "thinking" line while `busy`, and a
    /// 3-row input box at the bottom. `user_right` places the user's own
    /// messages on the right (default) or left.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        f: &mut Frame,
        area: Rect,
        focused: bool,
        busy: bool,
        elapsed_ms: u128,
        busy_tokens: u32,
        user_right: bool,
        borders: Borders,
    ) {
        let pal = palette();

        // Frames stay NEUTRAL gray — red/green are reserved for the editor's
        // diff gutter. Focus is a subtle brighten (ink), never the coral accent.
        let border_style = if focused {
            Style::default().fg(pal.ink)
        } else {
            Style::default().fg(pal.grid)
        };

        // NO top border ever; the caller passes BOTTOM (separates the chat from
        // the mode bar) + LEFT/RIGHT only for a sidebar that is actually open.
        let outer = Block::default().borders(borders).border_style(border_style);
        let inner = outer.inner(area);
        f.render_widget(outer, area);

        // A 1-row "thinking" line rides between history and input while busy.
        let spin_h = if busy { 1 } else { 0 };
        // Input box grows with the text: starts at 1 line, wraps up to 8. +1 for
        // the top rule that separates it from the history.
        let w = inner.width.max(1) as usize;
        let text_cells = 2 + self.input.chars().count() + 1; // "› " + input + cursor
        let input_lines = text_cells.div_ceil(w).clamp(1, 8) as u16;
        let input_h = input_lines + 1;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(spin_h), Constraint::Length(input_h)])
            .split(inner);
        let history_area = chunks[0];
        let spin_area = chunks[1];
        let input_area = chunks[2];

        // --- message history -------------------------------------------------
        let mut lines: Vec<Line> = Vec::new();
        // The banner is the FIRST row of the scrollable history — it shows at
        // the top when the chat is empty, then scrolls up out of view as
        // messages arrive (not a sticky top bar).
        if let Some(b) = self.banner.as_ref().filter(|s| !s.is_empty()) {
            lines.push(Line::from(Span::styled(b.clone(), Style::default().fg(pal.accent))));
            lines.push(Line::from(""));
        }
        for m in &self.msgs {
            // Tool action lines (the process log) — coloured like Claude Code:
            // the "> tool" in our accent2 (cyan/violet), file names in green,
            // errors in red; plain narration stays dim.
            if m.role == Role::Tool {
                let green = Color::Rgb(70, 200, 110);
                let red = Color::Rgb(235, 70, 70);
                let is_diff = |tok: &str| {
                    (tok.starts_with('+') || tok.starts_with('-'))
                        && tok.len() > 1
                        && tok[1..].chars().all(|c| c.is_ascii_digit())
                };
                for l in m.text.lines() {
                    if let Some(rest) = l.strip_prefix('●') {
                        // `● Tool(args) [+N -M]` — GREEN dot, bright name, dim
                        // args, coloured diff counts (Claude-Code style).
                        let rest = rest.trim_start();
                        let mut spans = vec![Span::styled("● ", Style::default().fg(green))];
                        match (rest.find('('), rest.rfind(')')) {
                            (Some(op), Some(cp)) if cp > op => {
                                spans.push(Span::styled(rest[..op].to_string(), Style::default().fg(pal.ink)));
                                spans.push(Span::styled(rest[op..=cp].to_string(), Style::default().fg(pal.muted)));
                                for tok in rest[cp + 1..].split_whitespace() {
                                    spans.push(Span::raw(" "));
                                    let c = if is_diff(tok) && tok.starts_with('+') { green }
                                        else if is_diff(tok) { red } else { pal.muted };
                                    spans.push(Span::styled(tok.to_string(), Style::default().fg(c)));
                                }
                            }
                            _ => {
                                // No args: `Name` or `Name +N -M`.
                                for (i, tok) in rest.split(' ').enumerate() {
                                    if i > 0 { spans.push(Span::raw(" ")); }
                                    let c = if is_diff(tok) && tok.starts_with('+') { green }
                                        else if is_diff(tok) { red }
                                        else if i == 0 { pal.ink } else { pal.muted };
                                    spans.push(Span::styled(tok.to_string(), Style::default().fg(c)));
                                }
                            }
                        }
                        lines.push(Line::from(spans).alignment(Alignment::Left));
                    } else if l.contains("error") {
                        lines.push(Line::from(Span::styled(l.to_string(), Style::default().fg(red))).alignment(Alignment::Left));
                    } else if l.starts_with('>') {
                        // Per-token colour: +N green (added), -N red (removed),
                        // the rest (> tool file) in accent2.
                        let mut spans: Vec<Span> = Vec::new();
                        for (i, tok) in l.split(' ').enumerate() {
                            if i > 0 { spans.push(Span::raw(" ")); }
                            let c = if tok.starts_with('+') && tok[1..].chars().all(|c| c.is_ascii_digit()) {
                                green
                            } else if tok.starts_with('-') && tok[1..].chars().all(|c| c.is_ascii_digit()) {
                                red
                            } else {
                                pal.accent2
                            };
                            spans.push(Span::styled(tok.to_string(), Style::default().fg(c)));
                        }
                        lines.push(Line::from(spans).alignment(Alignment::Left));
                    } else {
                        lines.push(Line::from(Span::styled(l.to_string(), Style::default().fg(pal.muted))).alignment(Alignment::Left));
                    }
                }
                continue;
            }
            let (label, label_style, text_style, align) = match m.role {
                Role::User => (
                    "you",
                    Style::default().fg(pal.accent),
                    Style::default().fg(pal.ink),
                    if user_right { Alignment::Right } else { Alignment::Left },
                ),
                Role::Agent => (
                    "agent",
                    Style::default().fg(pal.muted),
                    Style::default().fg(pal.ink),
                    Alignment::Left,
                ),
                Role::Tool => unreachable!("Tool handled above"),
            };

            // USER lines: plain GREEN text (variant A) — no fill, no "you" label.
            // AGENT lines: dim "agent" label + plain text.
            if m.role == Role::User {
                let fg = pal.done;
                let body: Vec<&str> = if m.text.is_empty() { vec![""] } else { m.text.lines().collect() };
                for l in body {
                    lines.push(
                        Line::from(Span::styled(l.to_string(), Style::default().fg(fg)))
                            .alignment(Alignment::Left),
                    );
                }
                let _ = (label, label_style, align);
            } else {
                lines.push(Line::from(Span::styled(label, label_style)).alignment(align));
                for l in m.text.lines() {
                    lines.push(Line::from(Span::styled(l.to_string(), text_style)).alignment(align));
                }
            }
        }

        if lines.is_empty() {
            lines.push(
                Line::from(Span::styled("(no messages yet)", Style::default().fg(pal.muted)))
                    .alignment(Alignment::Left),
            );
        }

        // Hard-wrap every line to the history width, THEN scroll. Doing the
        // wrap ourselves (instead of Paragraph::wrap + .scroll) fixes the
        // tail-scroll bug: with wrap on, `lines.len()` counts UN-wrapped lines
        // but ratatui displays wrapped rows, so `base = total - height`
        // undershoots and the newest lines slip below the visible bottom
        // ("chat won't scroll to the end"). Self-wrapping makes line count ==
        // displayed rows, so the scroll is exact.
        let flat = Self::hard_wrap_lines(lines, w);
        let history_h = history_area.height as usize;
        let total = flat.len();
        let base = total.saturating_sub(history_h) as u16;
        let scroll = base.saturating_sub(self.scroll.min(base));

        let history = Paragraph::new(flat).scroll((scroll, 0));
        f.render_widget(history, history_area);

        // --- thinking line: sphere + status + elapsed/token counter ----------
        // Like Claude Code's "✳ …ing (Xs · Y tokens)".
        if busy {
            let secs = (elapsed_ms / 1000) as u64;
            let clock = if secs >= 60 {
                format!("{}m {}s", secs / 60, secs % 60)
            } else {
                format!("{secs}s")
            };
            let spin = Line::from(vec![
                Span::styled(crate::sphere::line(elapsed_ms), Style::default().fg(pal.accent)),
                Span::styled(
                    format!("   {clock} · {busy_tokens} tok"),
                    Style::default().fg(pal.muted),
                ),
            ]);
            f.render_widget(Paragraph::new(spin), spin_area);
        }

        // --- input box: TOP rule only, sides flush with the chat column ------
        // The input rule is BRIGHT (ink) when the chat is focused, LIGHT GRAY
        // when you've left the chat — so it's obvious where the focus is.
        let input_border = if focused {
            Style::default().fg(pal.ink)
        } else {
            Style::default().fg(Color::Rgb(120, 124, 132))
        };
        let input_block = Block::default().borders(Borders::TOP).border_style(input_border);
        let input_inner = input_block.inner(input_area);
        f.render_widget(input_block, input_area);

        // Blinking block cursor — ONLY when the chat is focused (so you always
        // know whether you're typing here or somewhere else).
        let cursor_on = focused && (elapsed_ms / 450) % 2 == 0;
        let cursor = if cursor_on {
            Span::styled(" ", Style::default().bg(pal.ink))
        } else {
            Span::raw(" ")
        };
        let input_line = Line::from(vec![
            Span::styled("› ", Style::default().fg(pal.accent2)),
            Span::styled(self.input.as_str(), Style::default().fg(pal.ink)),
            cursor,
        ]);
        let input_para = Paragraph::new(input_line).wrap(Wrap { trim: false });
        f.render_widget(input_para, input_inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn hard_wrap_one_line_per_displayed_row_so_tail_scroll_is_exact() {
        // A 30-char line at width 10 → exactly 3 rows. line count == displayed
        // rows is what makes the tail-scroll offset correct (the bug was that
        // Paragraph::wrap + .scroll counted un-wrapped lines, undershooting).
        let lines = vec![Line::from(Span::raw("012345678901234567890123456789"))];
        let flat = ChatPane::hard_wrap_lines(lines, 10);
        assert_eq!(flat.len(), 3, "a 30-char line wraps to 3 rows at width 10");
        // Each row is at most the width.
        for row in &flat {
            let n: usize = row.spans.iter().map(|s| s.content.chars().count()).sum();
            assert!(n <= 10, "no row exceeds the width");
        }
    }

    #[test]
    fn hard_wrap_preserves_style_across_the_wrap_boundary() {
        let lines = vec![Line::from(Span::styled(
            "abcdefghijklmn",
            Style::default().fg(Color::Green),
        ))];
        let flat = ChatPane::hard_wrap_lines(lines, 5);
        assert_eq!(flat.len(), 3, "14 chars → 3 rows at width 5");
        // every span on every row keeps the green style.
        for row in &flat {
            for span in &row.spans {
                assert_eq!(span.style.fg, Some(Color::Green));
            }
        }
    }

    #[test]
    fn renders_without_panicking_and_input_roundtrips() {
        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).expect("terminal");

        let mut pane = ChatPane::new();
        pane.push(Role::User, "hello there, agent".to_string());
        pane.push(
            Role::Agent,
            "hi! how can I help you today with something long enough to wrap?".to_string(),
        );

        for c in "hi".chars() {
            pane.on_char(c);
        }
        pane.on_char('!');
        pane.backspace();

        // focused render
        terminal
            .draw(|f| {
                let area = f.area();
                pane.render(f, area, true, false, 0, 0, true, Borders::ALL);
            })
            .expect("draw focused");

        // unfocused render (different border style path)
        terminal
            .draw(|f| {
                let area = f.area();
                pane.render(f, area, false, false, 0, 0, true, Borders::ALL);
            })
            .expect("draw unfocused");

        // also exercise a tiny area to make sure nothing panics on cramped layout
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, 5, 2);
                pane.render(f, area, true, false, 0, 0, true, Borders::ALL);
            })
            .expect("draw tiny");

        assert_eq!(pane.take_input(), Some("hi".to_string()));
        assert_eq!(pane.take_input(), None);
        assert_eq!(pane.msgs.len(), 2);
    }

    #[test]
    fn empty_and_whitespace_input_is_not_taken() {
        let mut pane = ChatPane::new();
        assert_eq!(pane.take_input(), None);
        pane.on_char(' ');
        pane.on_char(' ');
        assert_eq!(pane.take_input(), None);
        assert_eq!(pane.input, "  ");
    }
}

