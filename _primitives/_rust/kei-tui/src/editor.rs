//! Minimal read/scroll code viewer + editor pane (t21).
//!
//! v1 is read-only: it opens a file from disk, displays it with a
//! line-number gutter, highlights the cursor line, and scrolls to keep
//! the cursor visible. Text mutation is deferred to a later iteration.

use std::cell::Cell;
use std::fs;
use std::path::PathBuf;

use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::theme::palette;

/// A minimal read/scroll code viewer + editor pane.
pub struct EditorPane {
    /// Path of the currently open file, if any.
    pub path: Option<PathBuf>,
    lines: Vec<String>,
    cursor: usize,
    /// Column (char index) of the caret within the cursor line.
    col: usize,
    scroll: u16,
    /// Unsaved edits present (shows a █ in the title; cleared on save).
    dirty: bool,
    /// Last known inner viewport height, cached from `render()` via
    /// interior mutability so `on_key` can keep the cursor in view (and
    /// size PageUp/PageDown) without needing the render area passed in.
    viewport_height: Cell<u16>,
}

impl Default for EditorPane {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorPane {
    /// Create an empty, unopened editor pane.
    pub fn new() -> Self {
        Self {
            path: None,
            lines: Vec::new(),
            cursor: 0,
            col: 0,
            scroll: 0,
            dirty: false,
            viewport_height: Cell::new(20),
        }
    }

    /// Read `path` into lines and reset cursor/scroll to the top.
    ///
    /// Never panics: on a read error the pane still opens with a single
    /// synthetic error line so the failure is visible in the UI.
    pub fn open(&mut self, path: PathBuf) {
        match fs::read_to_string(&path) {
            Ok(contents) => {
                self.lines = if contents.is_empty() {
                    vec![String::new()]
                } else {
                    contents.lines().map(|l| l.to_string()).collect()
                };
            }
            Err(err) => {
                self.lines = vec![format!("-- could not open {}: {}", path.display(), err)];
            }
        }
        self.path = Some(path);
        self.cursor = 0;
        self.col = 0;
        self.scroll = 0;
        self.dirty = false;
    }

    /// Whether a file (or an open-error placeholder) is currently loaded.
    pub fn is_open(&self) -> bool {
        self.path.is_some()
    }

    /// Close the file — the editor stops rendering and the chat reclaims the
    /// full center (Esc). Idempotent.
    pub fn close(&mut self) {
        self.path = None;
        self.lines.clear();
        self.cursor = 0;
        self.col = 0;
        self.scroll = 0;
        self.dirty = false;
    }

    /// Unsaved edits present.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Write the buffer back to its path (Ctrl-S). Returns the saved path on
    /// success. Best-effort — a write error is surfaced to the caller.
    pub fn save(&mut self) -> std::io::Result<PathBuf> {
        let Some(path) = self.path.clone() else {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no file open"));
        };
        let mut body = self.lines.join("\n");
        body.push('\n');
        fs::write(&path, body)?;
        self.dirty = false;
        Ok(path)
    }

    /// Clamp `col` to the current line's length.
    fn clamp_col(&mut self) {
        let len = self.lines.get(self.cursor).map(|l| l.chars().count()).unwrap_or(0);
        if self.col > len {
            self.col = len;
        }
    }

    /// Byte offset in the cursor line for the current `col` (char index).
    fn byte_at_col(line: &str, col: usize) -> usize {
        line.char_indices().nth(col).map(|(b, _)| b).unwrap_or(line.len())
    }

    /// Insert a typed character at the caret.
    fn insert_char(&mut self, c: char) {
        self.clamp_col();
        if let Some(line) = self.lines.get_mut(self.cursor) {
            let b = Self::byte_at_col(line, self.col);
            line.insert(b, c);
            self.col += 1;
            self.dirty = true;
        }
    }

    /// Split the line at the caret (Enter).
    fn newline(&mut self) {
        self.clamp_col();
        let line = self.lines.get(self.cursor).cloned().unwrap_or_default();
        let b = Self::byte_at_col(&line, self.col);
        let (head, tail) = line.split_at(b);
        self.lines[self.cursor] = head.to_string();
        self.lines.insert(self.cursor + 1, tail.to_string());
        self.cursor += 1;
        self.col = 0;
        self.dirty = true;
    }

    /// Delete the char before the caret, merging lines at column 0 (Backspace).
    fn backspace(&mut self) {
        self.clamp_col();
        if self.col > 0 {
            if let Some(line) = self.lines.get_mut(self.cursor) {
                let b = Self::byte_at_col(line, self.col - 1);
                line.remove(b);
                self.col -= 1;
                self.dirty = true;
            }
        } else if self.cursor > 0 {
            // Merge into the previous line.
            let cur = self.lines.remove(self.cursor);
            self.cursor -= 1;
            self.col = self.lines[self.cursor].chars().count();
            self.lines[self.cursor].push_str(&cur);
            self.dirty = true;
        }
    }

    /// Handle a key press: Up/Down move the cursor one line, PageUp/
    /// PageDown move it a full viewport, Home/End jump to the first/last
    /// line. Any other key is ignored. Scroll is adjusted afterwards so
    /// the cursor stays visible.
    pub fn on_key(&mut self, code: KeyCode) {
        if self.lines.is_empty() {
            return;
        }
        let last = self.lines.len().saturating_sub(1);
        match code {
            KeyCode::Up => {
                self.cursor = self.cursor.saturating_sub(1);
                self.clamp_col();
            }
            KeyCode::Down => {
                self.cursor = (self.cursor + 1).min(last);
                self.clamp_col();
            }
            KeyCode::Left => {
                if self.col > 0 {
                    self.col -= 1;
                } else if self.cursor > 0 {
                    self.cursor -= 1;
                    self.col = self.lines[self.cursor].chars().count();
                }
            }
            KeyCode::Right => {
                let len = self.lines[self.cursor].chars().count();
                if self.col < len {
                    self.col += 1;
                } else if self.cursor < last {
                    self.cursor += 1;
                    self.col = 0;
                }
            }
            KeyCode::PageUp => {
                let page = self.viewport_height.get().max(1) as usize;
                self.cursor = self.cursor.saturating_sub(page);
                self.clamp_col();
            }
            KeyCode::PageDown => {
                let page = self.viewport_height.get().max(1) as usize;
                self.cursor = (self.cursor + page).min(last);
                self.clamp_col();
            }
            KeyCode::Home => {
                self.cursor = 0;
                self.col = 0;
            }
            KeyCode::End => {
                self.cursor = last;
                self.clamp_col();
            }
            // --- mutation (the editor is editable) ---------------------------
            KeyCode::Char(c) => self.insert_char(c),
            KeyCode::Enter => self.newline(),
            KeyCode::Backspace => self.backspace(),
            _ => return,
        }
        self.adjust_scroll();
    }

    /// Keep `self.scroll` such that `self.cursor` stays within the last
    /// known viewport height.
    fn adjust_scroll(&mut self) {
        let height = self.viewport_height.get().max(1);
        let cursor = self.cursor as u16;
        if cursor < self.scroll {
            self.scroll = cursor;
        } else if cursor >= self.scroll + height {
            self.scroll = cursor - height + 1;
        }
        let max_scroll = (self.lines.len() as u16).saturating_sub(height);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }

    /// Render the pane: a bordered box titled with the open file's name
    /// (or " editor " when nothing is open), a muted line-number gutter,
    /// and the cursor line highlighted.
    pub fn render(&self, f: &mut Frame, area: Rect, focused: bool) {
        let pal = palette();

        let name = match &self.path {
            Some(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.display().to_string()),
            None => "editor".to_string(),
        };
        // A █ marks unsaved edits; when focused, hint the save key.
        let dot = if self.dirty { "█ " } else { "" };
        let tail = if focused { "  Ctrl-S save · Esc done" } else { "" };
        let title_text = format!(" {dot}{name} {tail} ");
        let title_line = Line::from(Span::styled(title_text, Style::default().fg(pal.ink)));

        let border_color = if focused { pal.ink } else { pal.grid };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(title_line);

        let inner = block.inner(area);
        f.render_widget(block, area);

        // Cache the viewport height so `on_key` can page/scroll correctly
        // even though it never sees the render area itself.
        self.viewport_height.set(inner.height.max(1));

        if self.lines.is_empty() {
            let empty = Paragraph::new(Line::from(Span::styled(
                "-- no file open --",
                Style::default().fg(pal.muted),
            )));
            f.render_widget(empty, inner);
            return;
        }

        let gutter_width = self.lines.len().to_string().len().max(3);
        let height = inner.height as usize;
        let start = (self.scroll as usize).min(self.lines.len());
        let end = (start + height).min(self.lines.len());

        let visible_lines: Vec<Line> = self.lines[start..end]
            .iter()
            .enumerate()
            .map(|(offset, text)| {
                let line_no = start + offset;
                let is_cursor = line_no == self.cursor;

                let num_span = Span::styled(
                    format!("{:>width$} ", line_no + 1, width = gutter_width),
                    Style::default().fg(pal.muted),
                );

                if is_cursor {
                    // Draw a block CARET at `col` — before | caret-char | after.
                    let caret = Style::default().fg(pal.paper).bg(pal.accent2).add_modifier(Modifier::BOLD);
                    let normal = Style::default().fg(pal.ink);
                    let chars: Vec<char> = text.chars().collect();
                    let c = self.col.min(chars.len());
                    let before: String = chars[..c].iter().collect();
                    let at: String = if c < chars.len() { chars[c].to_string() } else { " ".to_string() };
                    let after: String = if c < chars.len() { chars[c + 1..].iter().collect() } else { String::new() };
                    Line::from(vec![
                        num_span,
                        Span::styled(before, normal),
                        Span::styled(at, caret),
                        Span::styled(after, normal),
                    ])
                } else {
                    Line::from(vec![num_span, Span::styled(text.clone(), Style::default().fg(pal.ink))])
                }
            })
            .collect();

        let paragraph = Paragraph::new(visible_lines);
        f.render_widget(paragraph, inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn renders_empty_without_panicking() {
        let pane = EditorPane::new();
        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                pane.render(f, area, true);
            })
            .unwrap();
        assert!(!pane.is_open());
    }

    #[test]
    fn opens_missing_file_without_panicking() {
        let mut pane = EditorPane::new();
        pane.open(PathBuf::from("/nonexistent/path/does-not-exist.rs"));
        assert!(pane.is_open());
        assert_eq!(pane.lines.len(), 1);

        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                pane.render(f, area, false);
            })
            .unwrap();
    }

    #[test]
    fn cursor_and_scroll_move_and_clamp() {
        let mut pane = EditorPane::new();
        pane.lines = (0..100).map(|i| format!("line {}", i)).collect();
        pane.path = Some(PathBuf::from("virtual.mx"));

        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        // First render establishes the cached viewport height used by
        // on_key's paging/scroll math.
        terminal
            .draw(|f| {
                let area = f.area();
                pane.render(f, area, true);
            })
            .unwrap();

        for _ in 0..50 {
            pane.on_key(KeyCode::Down);
        }
        assert_eq!(pane.cursor, 50);
        assert!(pane.scroll > 0);

        for _ in 0..200 {
            pane.on_key(KeyCode::Down);
        }
        assert_eq!(pane.cursor, 99); // clamped to last line

        pane.on_key(KeyCode::Home);
        assert_eq!(pane.cursor, 0);
        assert_eq!(pane.scroll, 0);

        pane.on_key(KeyCode::End);
        assert_eq!(pane.cursor, 99);

        pane.on_key(KeyCode::PageUp);
        assert!(pane.cursor < 99);

        // Re-render after the moves to make sure nothing panics with a
        // non-zero scroll offset.
        terminal
            .draw(|f| {
                let area = f.area();
                pane.render(f, area, true);
            })
            .unwrap();
    }

    #[test]
    fn editing_inserts_splits_merges_and_saves() {
        let dir = std::env::temp_dir().join(format!("kei-editor-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("edit.txt");
        fs::write(&path, "ab\ncd\n").unwrap();
        let mut e = EditorPane::new();
        e.open(path.clone()); // lines = ["ab", "cd"], caret at (0,0)
        assert!(!e.is_dirty());
        // Insert 'X' > "Xab", caret at col 1
        e.on_key(KeyCode::Char('X'));
        assert!(e.is_dirty());
        assert_eq!(e.lines[0], "Xab");
        // Enter at col 1 splits "Xab" > "X" / "ab"
        e.on_key(KeyCode::Enter);
        assert_eq!(e.lines[0], "X");
        assert_eq!(e.lines[1], "ab");
        // Backspace at col 0 of line 1 merges it back into line 0 > "Xab"
        e.on_key(KeyCode::Backspace);
        assert_eq!(e.lines[0], "Xab");
        assert_eq!(e.lines[1], "cd");
        // Save round-trips to disk + clears dirty
        e.save().unwrap();
        assert!(!e.is_dirty());
        assert!(fs::read_to_string(&path).unwrap().starts_with("Xab\ncd"));
    }

    #[test]
    fn up_at_top_and_down_at_bottom_saturate() {
        let mut pane = EditorPane::new();
        pane.lines = vec!["only line".to_string()];
        pane.path = Some(PathBuf::from("one-liner.mx"));

        pane.on_key(KeyCode::Up);
        assert_eq!(pane.cursor, 0);
        pane.on_key(KeyCode::Down);
        assert_eq!(pane.cursor, 0);

        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                pane.render(f, area, true);
            })
            .unwrap();
    }
}

