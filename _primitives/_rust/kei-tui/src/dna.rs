//! DNA window — view + edit an agent's manifest (`_manifests/<name>.toml`), the
//! Constructor-Pattern SSoT for that agent. Full-screen (CenterMode::Dna),
//! reached by drilling `/agentslib` → an agent.
//!
//! The manifest is the agent's DNA: `name`, `description`, `model`,
//! `substrate_role` (scalar, EDITABLE here), plus `tools` / `blocks` /
//! `domain_in` (arrays, shown read-only for now). Editing a scalar rewrites just
//! that `key = "value"` line in place — a surgical line edit, never a full TOML
//! re-serialise (which would drop comments + reflow the arrays + role block).

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

/// The scalar fields the DNA window lets you edit, in display order.
const EDITABLE: &[&str] = &["name", "description", "model", "substrate_role"];

/// DNA window state: which agent, the raw manifest, a parsed view of the
/// editable scalars, the selection cursor, and (when editing) the in-progress
/// value.
#[derive(Debug, Clone, Default)]
pub struct DnaPane {
    pub agent: String,
    pub path: String,
    /// Raw manifest text (source of truth for surgical edits).
    pub raw: String,
    /// Load / read error, shown instead of fields.
    pub error: Option<String>,
    /// Cursor over the EDITABLE field list.
    pub selected: usize,
    /// `Some` while editing the selected field: the working value.
    pub editing: Option<String>,
}

impl DnaPane {
    /// Load agent `name`'s manifest from disk.
    pub fn open(&mut self, name: &str) {
        self.agent = name.to_string();
        self.path = format!("/home/keisei/work/KeiSeiKit-1.0/_manifests/{name}.toml");
        self.selected = 0;
        self.editing = None;
        match std::fs::read_to_string(&self.path) {
            Ok(s) => {
                self.raw = s;
                self.error = None;
            }
            Err(e) => {
                self.raw.clear();
                self.error = Some(format!("cannot read {}: {e}", self.path));
            }
        }
    }

    /// The current value of scalar `key` parsed from the raw manifest
    /// (`key = "..."`), or empty when absent.
    pub fn field(&self, key: &str) -> String {
        for line in self.raw.lines() {
            let t = line.trim_start();
            if let Some(rest) = t.strip_prefix(key) {
                let rest = rest.trim_start();
                if let Some(v) = rest.strip_prefix('=') {
                    return v.trim().trim_matches('"').to_string();
                }
            }
        }
        String::new()
    }

    pub fn move_up(&mut self) {
        if self.editing.is_none() {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    pub fn move_down(&mut self) {
        if self.editing.is_none() {
            self.selected = (self.selected + 1).min(EDITABLE.len() - 1);
        }
    }

    /// Begin editing the selected field (seed the buffer with its current value).
    pub fn begin_edit(&mut self) {
        if self.error.is_some() {
            return;
        }
        let key = EDITABLE[self.selected];
        self.editing = Some(self.field(key));
    }

    pub fn edit_char(&mut self, c: char) {
        if let Some(buf) = self.editing.as_mut() {
            buf.push(c);
        }
    }

    pub fn edit_backspace(&mut self) {
        if let Some(buf) = self.editing.as_mut() {
            buf.pop();
        }
    }

    /// Cancel the in-progress edit (Esc while editing).
    pub fn cancel_edit(&mut self) {
        self.editing = None;
    }

    /// Commit the in-progress edit: rewrite the selected `key = "..."` line in
    /// the raw manifest and persist it to disk. Returns Ok(key) on success.
    pub fn commit_edit(&mut self) -> Result<String, String> {
        let Some(val) = self.editing.take() else {
            return Err("not editing".into());
        };
        let key = EDITABLE[self.selected];
        let new_raw = rewrite_scalar(&self.raw, key, &val);
        std::fs::write(&self.path, &new_raw).map_err(|e| format!("write {}: {e}", self.path))?;
        self.raw = new_raw;
        Ok(key.to_string())
    }

    /// True while a field is being edited (drives the input caret).
    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    /// Render the DNA window full-screen into `area`.
    pub fn render(&self, f: &mut Frame, area: Rect) {
        let pal = crate::theme::palette();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(pal.grid))
            .title(format!(" DNA · {} ", self.agent));
        let inner = block.inner(area);
        f.render_widget(block, area);

        if let Some(err) = &self.error {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(err.clone(), Style::default().fg(pal.accent))))
                    .wrap(Wrap { trim: false }),
                inner,
            );
            return;
        }

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(Span::styled(
            "EDITABLE — ↑↓ field · Enter edit · Esc back",
            Style::default().fg(pal.muted).add_modifier(Modifier::UNDERLINED),
        )));
        for (i, key) in EDITABLE.iter().enumerate() {
            let selected = i == self.selected;
            let marker = if selected { "▸ " } else { "  " };
            // While editing the selected field, show the working buffer + caret.
            let value = if selected && self.editing.is_some() {
                format!("{}\u{2588}", self.editing.as_deref().unwrap_or(""))
            } else {
                self.field(key)
            };
            let key_fg = if selected { pal.done } else { pal.ink };
            lines.push(Line::from(vec![
                Span::styled(marker, Style::default().fg(pal.done)),
                Span::styled(format!("{key:<16}"), Style::default().fg(key_fg)),
                Span::styled(value, Style::default().fg(pal.ink)),
            ]));
        }

        // Read-only arrays below, for context.
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "READ-ONLY",
            Style::default().fg(pal.muted).add_modifier(Modifier::UNDERLINED),
        )));
        for key in ["tools", "blocks"] {
            let v = self.array_field(key);
            lines.push(Line::from(vec![
                Span::styled(format!("  {key:<16}"), Style::default().fg(pal.muted)),
                Span::styled(v, Style::default().fg(pal.muted)),
            ]));
        }

        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }

    /// A compact one-line rendering of a TOML array field (`key = [ ... ]`),
    /// best-effort — for the read-only context rows.
    fn array_field(&self, key: &str) -> String {
        let mut items: Vec<String> = Vec::new();
        let mut in_arr = false;
        let push_items = |s: &str, items: &mut Vec<String>| {
            for part in s.split(',') {
                let it = part.trim().trim_matches('"');
                if !it.is_empty() {
                    items.push(it.to_string());
                }
            }
        };
        for line in self.raw.lines() {
            let t = line.trim();
            if !in_arr {
                // Match `key = [`; the value may be on the same line (single-line
                // array) or span following lines.
                let head = format!("{key} ");
                let head2 = format!("{key}=");
                if (t.starts_with(&head) || t.starts_with(&head2)) && t.contains('[') {
                    let after = &t[t.find('[').unwrap() + 1..];
                    if let Some(end) = after.find(']') {
                        push_items(&after[..end], &mut items); // whole array on one line
                        break;
                    }
                    push_items(after, &mut items);
                    in_arr = true;
                }
                continue;
            }
            if let Some(end) = t.find(']') {
                push_items(&t[..end], &mut items);
                break;
            }
            push_items(t, &mut items);
        }
        items.join(" · ")
    }
}

/// Rewrite the first top-level `key = "..."` line in `raw` to `key = "val"`,
/// preserving everything else byte-for-byte. Appends the key if absent.
fn rewrite_scalar(raw: &str, key: &str, val: &str) -> String {
    let mut out = String::with_capacity(raw.len() + val.len());
    let mut done = false;
    for line in raw.lines() {
        if !done {
            let t = line.trim_start();
            if let Some(rest) = t.strip_prefix(key) {
                if rest.trim_start().starts_with('=') {
                    out.push_str(&format!("{key} = \"{val}\"\n"));
                    done = true;
                    continue;
                }
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if !done {
        out.push_str(&format!("{key} = \"{val}\"\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "name = \"researcher\"\nmodel = \"sonnet\"\ntools = [\"Glob\", \"Grep\"]\nsubstrate_role = \"read-only\"\n";

    fn pane() -> DnaPane {
        let mut p = DnaPane::default();
        p.raw = SAMPLE.to_string();
        p
    }

    #[test]
    fn field_parses_scalars() {
        let p = pane();
        assert_eq!(p.field("name"), "researcher");
        assert_eq!(p.field("model"), "sonnet");
        assert_eq!(p.field("substrate_role"), "read-only");
        assert_eq!(p.field("missing"), "");
    }

    #[test]
    fn rewrite_scalar_replaces_only_that_line() {
        let out = rewrite_scalar(SAMPLE, "model", "opus");
        assert!(out.contains("model = \"opus\""));
        assert!(out.contains("name = \"researcher\""), "other lines untouched");
        assert!(out.contains("tools = [\"Glob\", \"Grep\"]"), "arrays untouched");
        assert_eq!(out.matches("model = ").count(), 1, "no duplicate key");
    }

    #[test]
    fn rewrite_scalar_appends_when_absent() {
        let out = rewrite_scalar(SAMPLE, "description", "hello");
        assert!(out.contains("description = \"hello\""));
    }

    #[test]
    fn edit_flow_seeds_mutates_and_commits_into_raw() {
        let mut p = pane();
        // select "model" (index 2 in EDITABLE)
        p.selected = 2;
        assert_eq!(EDITABLE[2], "model");
        p.begin_edit();
        assert_eq!(p.editing.as_deref(), Some("sonnet"));
        p.editing = Some(String::new());
        for c in "opus".chars() {
            p.edit_char(c);
        }
        // commit rewrites raw (no disk in the test: point path at a temp file)
        let tmp = std::env::temp_dir().join("dna-edit-test.toml");
        p.path = tmp.to_string_lossy().into_owned();
        let key = p.commit_edit().expect("commit ok");
        assert_eq!(key, "model");
        assert_eq!(p.field("model"), "opus", "raw now reflects the edit");
        assert!(p.editing.is_none(), "edit buffer cleared");
    }

    #[test]
    fn array_field_joins_items() {
        let p = pane();
        assert_eq!(p.array_field("tools"), "Glob · Grep");
    }
}
