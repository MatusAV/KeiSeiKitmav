//! Settings panel (t22) — display + selection only.
//!
//! Renders a bordered list of editable configuration rows (theme, provider,
//! model, backend URL, autolaunch toggle). This pane owns nothing but the
//! currently-selected row index — the orchestrator is responsible for
//! interpreting a selection + subsequent action into an actual config
//! mutation, and for feeding this pane the live values to display.

use crossterm::event::KeyCode;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::theme::palette;

/// Number of rows rendered in the settings pane.
const ROW_COUNT: usize = 6;

/// The row index of the "you side" toggle (Enter / <> flips it).
pub const YOU_SIDE_ROW: usize = 5;

/// Settings panel state: which row is currently selected.
pub struct SettingsPane {
    pub sel: usize,
}

impl SettingsPane {
    /// Construct a fresh settings pane with the first row selected.
    pub fn new() -> Self {
        Self { sel: 0 }
    }

    /// Handle a key press: Up/Down move the selection, clamped to the
    /// available rows. All other keys are ignored (no-op).
    pub fn on_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up => {
                if self.sel > 0 {
                    self.sel -= 1;
                }
            }
            KeyCode::Down => {
                if self.sel + 1 < ROW_COUNT {
                    self.sel += 1;
                }
            }
            _ => {}
        }
    }

    /// Currently selected row index.
    pub fn selected(&self) -> usize {
        self.sel
    }

    /// Render the settings pane into `area`.
    ///
    /// `theme_name`, `provider`, and `base_url` are display-only values
    /// owned and supplied by the orchestrator; this pane never mutates
    /// them itself.
    pub fn render(
        &self,
        f: &mut Frame,
        area: Rect,
        focused: bool,
        theme_name: &str,
        provider: &str,
        base_url: &str,
        user_right: bool,
    ) {
        let pal = palette();

        let border_color = if focused { pal.ink } else { pal.grid };
        let block = Block::default()
            .title(" settings ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().fg(pal.ink));

        let rows = [
            format!("Theme:  {}  (F3 to cycle)", theme_name),
            format!("Provider:  {}", provider),
            "Model:  glm-4.7".to_string(),
            format!("Backend:  {}", base_url),
            "Autolaunch agent on start: off".to_string(),
            format!("Chat: you on the {}  (Enter / <>)", if user_right { "right" } else { "left" }),
        ];

        let items: Vec<ListItem> = rows
            .iter()
            .map(|row| ListItem::new(Line::from(row.as_str())))
            .collect();

        let list = List::new(items).block(block).highlight_style(
            Style::default()
                .bg(pal.accent)
                .fg(pal.paper)
                .add_modifier(Modifier::BOLD),
        );

        let mut state = ListState::default();
        state.select(Some(self.sel.min(ROW_COUNT - 1)));

        f.render_stateful_widget(list, area, &mut state);
    }
}

impl Default for SettingsPane {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn renders_without_panicking() {
        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let pane = SettingsPane::new();

        terminal
            .draw(|f| {
                let area = f.area();
                pane.render(f, area, true, "dark", "glm", "https://api.z.ai", true);
            })
            .unwrap();
    }

    #[test]
    fn renders_unfocused_at_a_non_zero_selection() {
        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut pane = SettingsPane::new();
        pane.on_key(KeyCode::Down);
        pane.on_key(KeyCode::Down);

        terminal
            .draw(|f| {
                let area = f.area();
                pane.render(f, area, false, "light", "openai", "http://localhost:8080", false);
            })
            .unwrap();
    }

    #[test]
    fn on_key_moves_and_clamps_selection() {
        let mut pane = SettingsPane::new();
        assert_eq!(pane.selected(), 0);

        pane.on_key(KeyCode::Up);
        assert_eq!(pane.selected(), 0, "cannot move above the first row");

        for _ in 0..(ROW_COUNT + 3) {
            pane.on_key(KeyCode::Down);
        }
        assert_eq!(
            pane.selected(),
            ROW_COUNT - 1,
            "cannot move past the last row"
        );

        pane.on_key(KeyCode::Up);
        assert_eq!(pane.selected(), ROW_COUNT - 2);
    }

    #[test]
    fn unrelated_keys_are_ignored() {
        let mut pane = SettingsPane::new();
        pane.on_key(KeyCode::Down);
        assert_eq!(pane.selected(), 1);

        pane.on_key(KeyCode::Char('x'));
        assert_eq!(pane.selected(), 1, "non-navigation keys are no-ops");
    }
}

