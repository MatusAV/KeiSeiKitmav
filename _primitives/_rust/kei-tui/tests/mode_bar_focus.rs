//! Focus gates on the bottom mode bar.
//!
//! Two regressions this pins:
//!   1. The chat's block caret kept blinking while the keyboard had moved down
//!      to the mode bar — a caret must only blink where you actually type.
//!   2. The selected control was signalled by colour alone. Selection is now a
//!      GREEN UNDERLINE (never BOLD — bold of a gray renders black here).

use kei_tui::app::App;
use kei_tui::types::Pane;
use kei_tui::ui::draw;
use ratatui::backend::TestBackend;
use ratatui::style::Modifier;
use ratatui::Terminal;

/// Draw one frame and hand back the buffer.
fn frame(app: &mut App) -> ratatui::buffer::Buffer {
    let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
    term.draw(|f| draw(f, app)).unwrap();
    term.backend().buffer().clone()
}

/// Does any cell carry the chat's block caret (a cell painted with the ink bg)?
fn has_caret(buf: &ratatui::buffer::Buffer) -> bool {
    let ink = kei_tui::theme::palette().ink;
    buf.content().iter().any(|c| c.bg == ink)
}

#[test]
fn caret_blinks_in_the_chat_when_the_chat_has_the_keyboard() {
    let mut app = App::new(std::env::temp_dir()).expect("init app");
    app.focus = Pane::Terminal; // the chat column
    app.bar_focus = None;
    assert!(has_caret(&frame(&mut app)), "chat is focused → caret is drawn");
}

#[test]
fn caret_is_gone_once_the_keyboard_moves_down_to_the_mode_bar() {
    let mut app = App::new(std::env::temp_dir()).expect("init app");
    app.focus = Pane::Terminal;
    app.bar_focus = Some(1); // navigated down onto `plan`
    assert!(
        !has_caret(&frame(&mut app)),
        "keyboard is on the mode bar → the chat caret must not blink"
    );
}

/// The `plan` label, and only it, is underlined when `plan` is the selection.
#[test]
fn the_selected_mode_bar_control_is_underlined_in_green() {
    let green = kei_tui::theme::palette().done;
    let mut app = App::new(std::env::temp_dir()).expect("init app");
    app.focus = Pane::Terminal;
    app.bar_focus = Some(1); // `plan`
    let buf = frame(&mut app);

    let underlined: Vec<&ratatui::buffer::Cell> = buf
        .content()
        .iter()
        .filter(|c| c.modifier.contains(Modifier::UNDERLINED))
        .collect();
    assert!(!underlined.is_empty(), "the selected control must be underlined");
    assert!(
        underlined.iter().all(|c| c.underline_color == green),
        "the underline under the selection is green"
    );

    // Selecting `plan` must not underline `auto` (the approval control).
    let underlined_text: String = underlined.iter().map(|c| c.symbol()).collect();
    assert!(underlined_text.contains("plan"), "the underline sits under `plan`");
    assert!(
        !underlined_text.contains("auto"),
        "only the selected control is underlined, not its neighbour"
    );
}

#[test]
fn nothing_is_underlined_while_the_bar_is_not_being_navigated() {
    let mut app = App::new(std::env::temp_dir()).expect("init app");
    app.focus = Pane::Terminal;
    app.bar_focus = None;
    let buf = frame(&mut app);
    assert!(
        !buf.content().iter().any(|c| c.modifier.contains(Modifier::UNDERLINED)),
        "no selection → no underline anywhere"
    );
}
