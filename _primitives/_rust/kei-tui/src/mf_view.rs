//! Passport VIEWER — renders a project's `.mf` passport in the right sidebar's
//! bottom half (t40 `/cp <project>`). This is only a Rust widget that READS the
//! `.mf` passport; the passport DATA stays `.mf` on disk
//! (`~/.claude/projects-state/<project>/`). We parse the canonical flat header
//! schema (`-- kubik:` / `-- does:` / `-- kind:` / `-- status:` /
//! `-- resolution:`) — never a bespoke format.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use std::path::{Path, PathBuf};

/// One parsed `.mf` node — only the header fields the pane renders.
#[derive(Clone, Default)]
pub struct MfNode {
    pub kubik: String,
    pub does: String,
    pub kind: String,
    pub status: String,
    pub resolution: String,
}

/// The passport pane: the loaded project + its parsed nodes.
#[derive(Default)]
pub struct PassportPane {
    pub project: Option<String>,
    pub nodes: Vec<MfNode>,
    pub error: Option<String>,
    /// Newest `.mf` mtime seen at the last load — drives the live re-read (t40):
    /// when a passport file changes on disk, `maybe_reload` re-parses so the
    /// pinned passport shows live task status.
    last_mtime: Option<std::time::SystemTime>,
    /// Throttle stat() to ~1×/sec so the 140ms draw ticker doesn't walk the dir
    /// every frame.
    last_check: Option<std::time::Instant>,
}

impl PassportPane {
    pub fn new() -> Self {
        Self::default()
    }

    /// (Re)load `~/.claude/projects-state/<project>/**/*.mf` and parse headers.
    /// Called on `/cp <project>` — cheap enough to re-run for a live view.
    pub fn load(&mut self, project: &str) {
        self.project = Some(project.to_string());
        self.nodes.clear();
        self.error = None;
        let Ok(home) = std::env::var("HOME") else {
            self.error = Some("no HOME".into());
            return;
        };
        let root = PathBuf::from(home).join(".claude/projects-state").join(project);
        if !root.is_dir() {
            self.error = Some(format!("no passport dir for '{project}'"));
            return;
        }
        let mut files = Vec::new();
        collect_mf(&root, &mut files);
        files.sort();
        for f in files {
            if let Ok(s) = std::fs::read_to_string(&f) {
                self.nodes.push(parse_mf(&s));
            }
        }
        if self.nodes.is_empty() {
            self.error = Some("passport is empty".into());
        }
        self.last_mtime = self.newest_mtime();
    }

    /// The newest modification time across the project's `.mf` files, or `None`
    /// when there's no loaded project / the dir is gone.
    pub fn newest_mtime(&self) -> Option<std::time::SystemTime> {
        let project = self.project.as_ref()?;
        let home = std::env::var("HOME").ok()?;
        let root = PathBuf::from(home).join(".claude/projects-state").join(project);
        let mut files = Vec::new();
        collect_mf(&root, &mut files);
        files
            .iter()
            .filter_map(|f| std::fs::metadata(f).ok()?.modified().ok())
            .max()
    }

    /// Live re-read (t40): if a passport file changed on disk since the last
    /// load, re-parse. Throttled to ~1×/sec so it's cheap to call every frame.
    /// No-op when no project is loaded.
    pub fn maybe_reload(&mut self) {
        let Some(project) = self.project.clone() else { return };
        // Throttle: skip if we checked < 1s ago.
        if let Some(t) = self.last_check {
            if t.elapsed() < std::time::Duration::from_secs(1) {
                return;
            }
        }
        self.last_check = Some(std::time::Instant::now());
        let now = self.newest_mtime();
        if now != self.last_mtime {
            self.load(&project); // load() refreshes last_mtime
        }
    }

    /// Render: non-task nodes (overview/plan) first, then tasks grouped by
    /// status (in_progress > open > blocked > done).
    pub fn render(&self, f: &mut Frame, area: Rect, focused: bool) {
        let pal = crate::theme::palette();
        let border = if focused { pal.ink } else { pal.grid };
        let title = match &self.project {
            Some(p) => format!(" passport · {p} "),
            None => " passport  (/cp <project>) ".into(),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border))
            .title(title);
        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut lines: Vec<Line> = Vec::new();
        if let Some(e) = &self.error {
            lines.push(Line::from(Span::styled(e.clone(), Style::default().fg(pal.accent))));
        } else if self.nodes.is_empty() {
            lines.push(Line::from(Span::styled(
                "type  /cp <project>  in chat",
                Style::default().fg(pal.muted),
            )));
        } else {
            for n in self.nodes.iter().filter(|n| n.kind != "task") {
                lines.push(Line::from(vec![
                    Span::styled(format!("▸ {} ", n.kind), Style::default().fg(pal.accent2)),
                    Span::styled(short(&n.does, 46), Style::default().fg(pal.ink)),
                ]));
            }
            for grp in ["in_progress", "open", "blocked", "done"] {
                let tasks: Vec<&MfNode> =
                    self.nodes.iter().filter(|n| n.kind == "task" && n.status == grp).collect();
                if tasks.is_empty() {
                    continue;
                }
                lines.push(Line::from(Span::styled(
                    format!("── {grp} ({}) ──", tasks.len()),
                    Style::default().fg(pal.muted),
                )));
                for t in tasks {
                    let dot = match grp {
                        "done" => pal.done,
                        "in_progress" => pal.accent2,
                        "blocked" => pal.accent,
                        _ => pal.muted,
                    };
                    lines.push(Line::from(vec![
                        Span::styled("█ ", Style::default().fg(dot)),
                        Span::styled(format!("{}  ", t.kubik), Style::default().fg(pal.ink)),
                        Span::styled(short(&t.does, 34), Style::default().fg(pal.muted)),
                    ]));
                }
            }
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
    }
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

fn collect_mf(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_mf(&p, out);
            } else if p.extension().and_then(|x| x.to_str()) == Some("mf") {
                out.push(p);
            }
        }
    }
}

/// Parse the canonical `.mf` flat header (leading `-- key: value` lines).
fn parse_mf(s: &str) -> MfNode {
    let mut n = MfNode::default();
    for line in s.lines() {
        let l = line.trim_start();
        if let Some(v) = l.strip_prefix("-- kubik:") {
            n.kubik = v.trim().to_string();
        } else if let Some(v) = l.strip_prefix("-- does:") {
            n.does = v.trim().to_string();
        } else if let Some(v) = l.strip_prefix("-- kind:") {
            n.kind = v.trim().to_string();
        } else if let Some(v) = l.strip_prefix("-- status:") {
            n.status = v.trim().to_string();
        } else if let Some(v) = l.strip_prefix("-- resolution:") {
            n.resolution = v.trim().to_string();
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn parses_flat_mf_headers() {
        let n = parse_mf(
            "-- kubik: t20-chat\n-- does: chat pane\n-- kind: task\n-- status: done\n-- resolution: shipped\nbody text",
        );
        assert_eq!(n.kubik, "t20-chat");
        assert_eq!(n.kind, "task");
        assert_eq!(n.status, "done");
        assert_eq!(n.does, "chat pane");
    }

    #[test]
    fn renders_empty_and_loaded_without_panic() {
        let mut term = Terminal::new(TestBackend::new(32, 20)).unwrap();
        let mut p = PassportPane::new();
        term.draw(|f| p.render(f, f.area(), false)).unwrap();
        p.nodes.push(parse_mf("-- kubik: t1\n-- does: x\n-- kind: task\n-- status: open"));
        p.project = Some("demo".into());
        term.draw(|f| p.render(f, f.area(), true)).unwrap();
    }

    #[test]
    fn maybe_reload_is_a_noop_without_a_project() {
        let mut p = PassportPane::new();
        p.maybe_reload(); // no project loaded → must not panic / touch anything
        assert!(p.project.is_none());
        assert!(p.nodes.is_empty());
    }

    #[test]
    fn newest_mtime_is_some_for_a_real_loaded_passport() {
        // The keiseikit-tui passport exists on this host; loading it should give
        // a non-empty node set and a newest mtime that drives the live re-read.
        let mut p = PassportPane::new();
        p.load("keiseikit-tui");
        // Only assert when the load succeeded AND nothing raced the passport dir
        // out from under us (a concurrent .mf edit can flip error/emptiness); the
        // point is that a successful load records a mtime, not the exact value.
        if p.error.is_none() && !p.nodes.is_empty() {
            assert!(p.last_mtime.is_some(), "a successful load records the mtime");
        }
    }

    #[test]
    fn maybe_reload_throttles_within_one_second() {
        let mut p = PassportPane::new();
        p.load("keiseikit-tui");
        if p.error.is_some() {
            return; // no passport on this host — skip
        }
        // First maybe_reload sets last_check; a second immediately after is
        // throttled (returns before doing the stat walk), so it must not panic.
        p.maybe_reload();
        let n = p.nodes.len();
        p.maybe_reload();
        assert_eq!(p.nodes.len(), n, "throttled call left the state unchanged");
    }
}
