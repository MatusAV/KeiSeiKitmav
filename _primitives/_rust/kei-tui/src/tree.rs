//! Lazy file-tree pane for the kei-tui ratatui/crossterm interface.
//!
//! Pure `std::fs` — no async, no extra crates. Directories are read on demand
//! the first time they are expanded; the visible rows are rebuilt as a flat
//! `(path, depth, is_dir, expanded)` list on every expand/collapse, and the
//! selection is re-resolved by path so it survives the rebuild.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

/// A single visible row in the flattened tree.
struct Row {
    path: PathBuf,
    depth: usize,
    is_dir: bool,
    expanded: bool,
}

/// A lazy, navigable file-tree pane.
pub struct TreePane {
    root: PathBuf,
    /// Directory paths whose children are currently visible.
    expanded: HashSet<PathBuf>,
    /// Flattened visible rows, rebuilt on every expand/collapse.
    rows: Vec<Row>,
    /// Index into `rows` of the highlighted entry.
    selected: usize,
}

impl TreePane {
    /// Root defaults to `.` if the path is unreadable.
    pub fn new(root: PathBuf) -> Self {
        let root = if fs::read_dir(&root).is_ok() {
            root
        } else {
            PathBuf::from(".")
        };
        let mut expanded = HashSet::new();
        // Root starts expanded so the pane shows content immediately — this
        // first expansion is exactly the lazy read_dir that populates it.
        expanded.insert(root.clone());
        let mut pane = TreePane {
            expanded,
            rows: Vec::new(),
            selected: 0,
            root,
        };
        pane.rebuild();
        pane
    }

    /// Render the tree as a bordered pane titled " files "; Cyan border when
    /// `focused`, DarkGray otherwise. ▾/▸ marks expanded/collapsed dirs, rows
    /// are indented by depth, and the selected row is highlighted (Black on Cyan).
    pub fn render(&mut self, f: &mut Frame, area: Rect, focused: bool) {
        let border_color = if focused { crate::theme::palette().ink } else { crate::theme::palette().grid };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" files ")
            .border_style(Style::default().fg(border_color));

        let items = self
            .rows
            .iter()
            .map(|row| {
                let indent = "  ".repeat(row.depth);
                let name = row
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| row.path.to_string_lossy().into_owned());
                // Font Awesome glyphs (monochrome, not emoji): folder / folder-open / file.
                // Rendered via fontconfig fallback (FontAwesome.otf is installed).
                let (icon, icon_style) = if row.is_dir {
                    let g = if row.expanded { "\u{f07c}" } else { "\u{f07b}" };
                    (g, Style::default().fg(crate::theme::palette().accent2))
                } else {
                    ("\u{f016}", Style::default().fg(crate::theme::palette().muted))
                };
                ListItem::new(Line::from(vec![
                    Span::raw(indent),
                    Span::styled(icon, icon_style),
                    Span::raw(" "),
                    Span::raw(name),
                ]))
            })
            .collect::<Vec<_>>();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(crate::theme::palette().accent).fg(crate::theme::palette().paper));

        let mut state = ListState::default();
        if !self.rows.is_empty() {
            state.select(Some(self.selected));
        }
        f.render_stateful_widget(list, area, &mut state);
    }

    /// Up/Down: move selection (clamp). Right or Enter on a dir: lazily
    /// read_dir + expand (Right also descends into the first child). Left:
    /// collapse the dir, or move to the selected item's parent. Other keys
    /// are ignored.
    pub fn on_key(&mut self, code: KeyCode) {
        if self.rows.is_empty() {
            return;
        }
        match code {
            KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.selected + 1 < self.rows.len() {
                    self.selected += 1;
                }
            }
            KeyCode::Right => {
                let (is_dir, was_expanded, depth, path) = self.snapshot_selected();
                if !is_dir {
                    return;
                }
                if !was_expanded {
                    self.expanded.insert(path);
                    self.rebuild();
                }
                // descend to the first child, if any
                let child_depth = depth + 1;
                if self.selected + 1 < self.rows.len()
                    && self.rows[self.selected + 1].depth == child_depth
                {
                    self.selected += 1;
                }
            }
            KeyCode::Enter => {
                let (is_dir, was_expanded, _depth, path) = self.snapshot_selected();
                if is_dir && !was_expanded {
                    self.expanded.insert(path);
                    self.rebuild();
                }
            }
            KeyCode::Left => {
                let (is_dir, was_expanded, depth, path) = self.snapshot_selected();
                if is_dir && was_expanded {
                    self.expanded.remove(&path);
                    self.rebuild();
                } else if depth > 0 {
                    // jump to the selected item's parent directory: the nearest
                    // ancestor above, which sits at exactly depth - 1.
                    let target = depth - 1;
                    let mut i = self.selected;
                    while i > 0 {
                        i -= 1;
                        if self.rows[i].depth == target {
                            self.selected = i;
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// The root directory as a display string (for the header cwd line).
    pub fn root_label(&self) -> String {
        self.root.display().to_string()
    }

    /// Absolute path of the currently selected node, if any.
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.rows.get(self.selected).map(|r| r.path.clone())
    }

    /// Select the row at visible index `i` (clamped). Used for mouse clicks.
    pub fn select_index(&mut self, i: usize) {
        if !self.rows.is_empty() {
            self.selected = i.min(self.rows.len() - 1);
        }
    }

    /// Move the selection (and thus the visible window) by `n` rows — mouse
    /// wheel over the file tree.
    pub fn scroll(&mut self, down: bool, n: usize) {
        if self.rows.is_empty() {
            return;
        }
        if down {
            self.selected = (self.selected + n).min(self.rows.len() - 1);
        } else {
            self.selected = self.selected.saturating_sub(n);
        }
    }

    /// Is the currently selected row a directory?
    pub fn selected_is_dir(&self) -> bool {
        self.rows.get(self.selected).map(|r| r.is_dir).unwrap_or(false)
    }

    /// Toggle expand/collapse of the selected directory (no-op on files).
    pub fn toggle_selected(&mut self) {
        let (is_dir, was_expanded, _depth, path) = self.snapshot_selected();
        if !is_dir {
            return;
        }
        if was_expanded {
            self.expanded.remove(&path);
        } else {
            self.expanded.insert(path);
        }
        self.rebuild();
    }

    /// Copy the selected row's salient fields out so we can mutate `self`
    /// (insert into / remove from `expanded` + rebuild) without a borrow fight.
    fn snapshot_selected(&self) -> (bool, bool, usize, PathBuf) {
        let r = &self.rows[self.selected];
        (r.is_dir, r.expanded, r.depth, r.path.clone())
    }

    /// Rebuild the flattened visible-row list from the root, descending only
    /// into expanded directories. Selection is re-resolved by path so it is
    /// preserved across the structural change (and clamped as a safety net).
    fn rebuild(&mut self) {
        let sel_path = self.rows.get(self.selected).map(|r| r.path.clone());
        self.rows.clear();
        let root = self.root.clone();
        let root_is_dir = root.is_dir();
        self.expand_into(&root, root_is_dir, 0);

        self.selected = match sel_path
            .as_ref()
            .and_then(|sp| self.rows.iter().position(|r| &r.path == sp))
        {
            Some(i) => i,
            None => 0,
        };
        if !self.rows.is_empty() && self.selected >= self.rows.len() {
            self.selected = self.rows.len() - 1;
        }
        if self.rows.is_empty() {
            self.selected = 0;
        }
    }

    /// Push `path` as a row and — if it is an expanded directory — read its
    /// children (dirs first, then alphabetical) and recurse into them.
    /// Any fs error short-circuits silently: never panics.
    fn expand_into(&mut self, path: &Path, is_dir: bool, depth: usize) {
        let is_expanded = self.expanded.contains(path);
        self.rows.push(Row {
            path: path.to_path_buf(),
            depth,
            is_dir,
            expanded: is_expanded,
        });
        if !is_dir || !is_expanded {
            return;
        }
        let mut kids: Vec<(PathBuf, bool)> = match fs::read_dir(path) {
            Ok(rd) => rd
                .flatten()
                .map(|e| {
                    let p = e.path();
                    let d = p.is_dir();
                    (p, d)
                })
                .collect(),
            Err(_) => return,
        };
        kids.sort_by(|a, b| match (a.1, b.1) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => a.0.file_name().cmp(&b.0.file_name()),
        });
        for (child_path, child_is_dir) in kids {
            self.expand_into(&child_path, child_is_dir, depth + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn tree_pane_renders_and_navigates_without_panic() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut pane = TreePane::new(std::env::temp_dir());

        let area = ratatui::layout::Rect::new(0, 0, 100, 30); // ratatui 0.29: size()>Size, use a Rect
        // render must not panic in either focus state
        terminal.draw(|f| pane.render(f, area, true)).unwrap();
        terminal.draw(|f| pane.render(f, area, false)).unwrap();

        // after construction the root is selected and reported as an absolute path
        let sel = pane.selected_path().expect("root should be selected");
        assert!(sel.is_absolute(), "selected path must be absolute");

        // navigation must not panic regardless of tree contents / focus
        for code in [
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Up,
            KeyCode::Right,
            KeyCode::Enter,
            KeyCode::Left,
            KeyCode::Left,
            KeyCode::Up,
            KeyCode::Char('x'),
        ] {
            pane.on_key(code);
        }

        // the root row is never removed, so a selection always resolves and is
        // within range (selected_path sanity)
        assert!(pane.selected_path().is_some());
        assert!(pane.selected < pane.rows.len() || pane.rows.is_empty());
    }
}
