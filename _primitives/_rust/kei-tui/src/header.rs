//! The persistent top statusline — ONE row, like Claude Code's banner: a small
//! spinning Frobenius-sphere marker, then KEISEIKODE + version + model/provider/
//! effort + cwd, all inline. The chat rides below it on the freed rows.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Version shown in the statusline.
pub const VERSION: &str = "v0.1";

/// Render the statusline into a 1-row `area`: `◐ KEISEIKODE v0.1 · model ·
/// provider · effort · cwd`. The leading marker is the spinning Frobenius
/// sphere (`sphere::line`, animated each frame) — the logo, kept compact so the
/// header takes a single row instead of the old 6-row pixel-art block.
pub fn render(f: &mut Frame, area: Rect, elapsed_ms: u128, model: &str, provider: &str, effort: &str, cwd: &str) {
    let pal = crate::theme::palette();
    let line = Line::from(vec![
        Span::styled(crate::sphere::line(elapsed_ms), Style::default().fg(pal.accent)),
        Span::raw(" "),
        Span::styled("KEISEIKODE", Style::default().fg(pal.accent)),
        Span::styled(format!(" {VERSION}"), Style::default().fg(pal.muted)),
        Span::styled("  ·  ", Style::default().fg(pal.muted)),
        Span::styled(model.to_string(), Style::default().fg(pal.done)),
        Span::styled(format!(" · {provider} · {effort}"), Style::default().fg(pal.muted)),
        Span::styled("  ·  ", Style::default().fg(pal.muted)),
        Span::styled(cwd.to_string(), Style::default().fg(pal.muted)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    // render is pure + allocation-free; the contract is "one row, no panic".
    // (The pixel-art sphere_lines helper was removed with the 6-row header.)
    #[test]
    fn render_one_row_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render(f, f.area(), 1234, "glm-4.6", "glm-zai", "medium", "/tmp"))
            .unwrap();
    }
}
