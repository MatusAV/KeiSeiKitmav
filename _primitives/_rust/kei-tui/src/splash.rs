//! Startup splash: "KEISEI" over "KODE" in the ANSI-Shadow block font (the same
//! one the `keisei` launcher uses) — the box-drawing edges (╗╔╝═╚║) give each
//! letter built-in 3D depth. We colour the solid faces (█) RED and those edges
//! GREEN, so it reads as "красные буквы, зелёная тень". ~1.5s, skippable.

use ratatui::backend::Backend;
use ratatui::layout::Alignment;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Terminal;
use std::time::Duration;

/// 6-row ANSI-Shadow glyph for the letters we need (K E I S O D).
fn glyph(c: char) -> [&'static str; 6] {
    match c {
        'K' => ["██╗  ██╗", "██║ ██╔╝", "█████╔╝ ", "██╔═██╗ ", "██║  ██╗", "╚═╝  ╚═╝"],
        'E' => ["███████╗", "██╔════╝", "█████╗  ", "██╔══╝  ", "███████╗", "╚══════╝"],
        'I' => ["██╗", "██║", "██║", "██║", "██║", "╚═╝"],
        'S' => ["███████╗", "██╔════╝", "███████╗", "╚════██║", "███████║", "╚══════╝"],
        'O' => [" ██████╗ ", "██╔═══██╗", "██║   ██║", "██║   ██║", "╚██████╔╝", " ╚═════╝ "],
        'D' => ["██████╗ ", "██╔══██╗", "██║  ██║", "██║  ██║", "██████╔╝", "╚═════╝ "],
        'C' => [" ██████╗", "██╔════╝", "██║     ", "██║     ", "╚██████╗", " ╚═════╝"],
        _ => ["  ", "  ", "  ", "  ", "  ", "  "],
    }
}

/// Join a word's per-letter glyph rows with a 1-space gutter > 6 strings.
fn word_rows(word: &str) -> Vec<String> {
    let glyphs: Vec<[&str; 6]> = word.chars().map(glyph).collect();
    (0..6)
        .map(|r| glyphs.iter().map(|g| g[r]).collect::<Vec<_>>().join(" "))
        .collect()
}

/// Colour one glyph row: solid faces (█) RED, edges (box-drawing) GREEN.
fn colored_row(row: &str, red: Color, green: Color) -> Line<'static> {
    let spans: Vec<Span> = row
        .chars()
        .map(|ch| {
            let color = if ch == '█' {
                red
            } else if ch == ' ' {
                return Span::raw(" ");
            } else {
                green // ╗ ╔ ╝ ╚ ═ ║ — the 3D edge / shadow
            };
            Span::styled(ch.to_string(), Style::default().fg(color))
        })
        .collect();
    Line::from(spans).alignment(Alignment::Center)
}

/// Build the centered banner: KEISEI over `second`, coloured red/green.
fn banner(second: &str, red: Color, green: Color) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = vec![Line::from("")];
    for r in word_rows("KEISEI") {
        lines.push(colored_row(&r, red, green));
    }
    lines.push(Line::from(""));
    for r in word_rows(second) {
        lines.push(colored_row(&r, red, green));
    }
    lines
}

fn show<B: Backend>(terminal: &mut Terminal<B>, lines: &[Line<'static>]) {
    let _ = terminal.draw(|f| {
        let area = f.area();
        let pad = (area.height as usize).saturating_sub(lines.len() + 1) / 2;
        let mut all: Vec<Line> = (0..pad).map(|_| Line::from("")).collect();
        all.extend(lines.iter().cloned());
        f.render_widget(Paragraph::new(all).alignment(Alignment::Center), area);
    });
}

/// Play the splash: KEISEI over **CODE**, then the C morphs to K > **KODE**
/// (KeiSeiKode). Skippable on any keypress.
pub async fn play<B: Backend>(terminal: &mut Terminal<B>) {
    let red = Color::Rgb(232, 62, 62);
    let green = Color::Rgb(42, 190, 92);
    show(terminal, &banner("CODE", red, green));
    if skip_wait(650).await {
        return;
    }
    show(terminal, &banner("KODE", red, green));
    let _ = skip_wait(900).await;
}

/// Sleep `ms`, returning true early if a key is pressed (drains it).
async fn skip_wait(ms: u64) -> bool {
    let step = 30u64;
    let mut left = ms;
    while left > 0 {
        if crossterm::event::poll(Duration::from_millis(0)).unwrap_or(false) {
            let _ = crossterm::event::read();
            return true;
        }
        let d = step.min(left);
        tokio::time::sleep(Duration::from_millis(d)).await;
        left -= d;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_rows_are_six_and_nonempty() {
        let r = word_rows("KEISEI");
        assert_eq!(r.len(), 6);
        assert!(r.iter().all(|s| !s.is_empty()));
        assert!(word_rows("KODE")[0].contains('█'));
    }

    #[test]
    fn banner_has_both_words() {
        let b = banner("KODE", Color::Red, Color::Green);
        // 1 blank + 6 KEISEI + 1 blank + 6 KODE = 14 lines
        assert_eq!(b.len(), 14);
    }
}
