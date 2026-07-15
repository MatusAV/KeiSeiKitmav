//! Embedded shell PTY pane (t02).
//!
//! Architecture (the tui-term pattern): `portable-pty` opens a PTY and spawns
//! `$SHELL`; a reader thread feeds the raw bytes into a `vt100::Parser` (an
//! in-memory screen grid); `tui-term`'s `PseudoTerminal` widget renders that
//! grid inside a ratatui pane each frame. Keystrokes are translated to bytes
//! and written back to the PTY. Input routing is the app's job — this pane only
//! exposes `on_key` / `feed_str` and never steals events on its own.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

/// Map a vt100 cell colour to a ratatui colour.
fn conv_color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Blit a vt100 screen grid into a ratatui buffer region, cell by cell.
fn blit_screen(screen: &vt100::Screen, area: Rect, buf: &mut Buffer) {
    let (rows, cols) = screen.size();
    for r in 0..rows.min(area.height) {
        for c in 0..cols.min(area.width) {
            let Some(cell) = screen.cell(r, c) else { continue };
            let target = &mut buf[(area.x + c, area.y + r)];
            let s = cell.contents();
            target.set_symbol(if s.is_empty() { " " } else { &s });
            let mut style = Style::default()
                .fg(conv_color(cell.fgcolor()))
                .bg(conv_color(cell.bgcolor()));
            if cell.bold() {
                style = style.add_modifier(Modifier::BOLD);
            }
            if cell.italic() {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if cell.underline() {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            if cell.inverse() {
                style = style.add_modifier(Modifier::REVERSED);
            }
            target.set_style(style);
        }
    }
}

/// A live embedded terminal: shell child + shared vt100 screen + PTY writer.
pub struct TerminalPane {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    rows: u16,
    cols: u16,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl TerminalPane {
    /// Spawn `$SHELL` (fallback `/bin/sh`) in a `rows`x`cols` PTY rooted at `cwd`.
    pub fn new(rows: u16, cols: u16, cwd: &std::path::Path) -> Result<Self> {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .context("openpty")?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let mut cmd = CommandBuilder::new(shell);
        cmd.cwd(cwd);
        let child = pair.slave.spawn_command(cmd).context("spawn shell")?;
        drop(pair.slave); // release the slave fd; the child holds its own

        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let mut reader = pair.master.try_clone_reader().context("clone pty reader")?;
        {
            let parser = parser.clone();
            thread::spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break, // EOF or error > shell exited
                        Ok(n) => {
                            if let Ok(mut p) = parser.lock() {
                                p.process(&buf[..n]);
                            }
                        }
                    }
                }
            });
        }
        let writer = pair.master.take_writer().context("take pty writer")?;
        Ok(Self {
            parser,
            writer,
            master: pair.master,
            rows,
            cols,
            _child: child,
        })
    }

    /// Resize the PTY + parser (call with the pane's inner size before render).
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        let _ = self
            .master
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        if let Ok(mut p) = self.parser.lock() {
            p.set_size(rows, cols);
        }
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// Insert a literal string into the shell input — t04 drops a file path here.
    pub fn feed_str(&mut self, s: &str) {
        self.write_bytes(s.as_bytes());
    }

    /// Translate a key event to a byte sequence and forward it to the shell.
    pub fn on_key(&mut self, key: KeyEvent) {
        let bytes: Vec<u8> = match key.code {
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    let up = c.to_ascii_uppercase();
                    if up.is_ascii_uppercase() {
                        vec![(up as u8) - b'A' + 1] // Ctrl-A..Ctrl-Z > 0x01..0x1a
                    } else {
                        vec![c as u8]
                    }
                } else {
                    let mut b = [0u8; 4];
                    c.encode_utf8(&mut b).as_bytes().to_vec()
                }
            }
            KeyCode::Enter => vec![b'\r'],
            KeyCode::Backspace => vec![0x7f],
            KeyCode::Tab => vec![b'\t'],
            KeyCode::Esc => vec![0x1b],
            KeyCode::Up => b"\x1b[A".to_vec(),
            KeyCode::Down => b"\x1b[B".to_vec(),
            KeyCode::Right => b"\x1b[C".to_vec(),
            KeyCode::Left => b"\x1b[D".to_vec(),
            KeyCode::Home => b"\x1b[H".to_vec(),
            KeyCode::End => b"\x1b[F".to_vec(),
            KeyCode::Delete => b"\x1b[3~".to_vec(),
            _ => return,
        };
        self.write_bytes(&bytes);
    }

    /// Render the terminal grid inside a bordered pane; cyan border when focused.
    pub fn render(&mut self, f: &mut Frame, area: Rect, focused: bool, title: &str) {
        let inner_rows = area.height.saturating_sub(2);
        let inner_cols = area.width.saturating_sub(2);
        self.resize(inner_rows, inner_cols);
        let color = if focused { crate::theme::palette().accent } else { crate::theme::palette().grid };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {title} "))
            .border_style(Style::default().fg(color));
        let inner = block.inner(area);
        f.render_widget(block, area);
        if let Ok(p) = self.parser.lock() {
            blit_screen(p.screen(), inner, f.buffer_mut());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn spawns_shell_and_renders_without_panic() {
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let mut pane = TerminalPane::new(22, 78, std::path::Path::new("/tmp")).expect("spawn pty");
        pane.feed_str("echo hi\n");
        std::thread::sleep(std::time::Duration::from_millis(150));
        term.draw(|f| pane.render(f, f.area(), true, "terminal")).unwrap();
    }
}
