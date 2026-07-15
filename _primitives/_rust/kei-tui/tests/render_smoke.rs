//! Headless render gate: draw one frame to a TestBackend (no tty needed) and
//! assert the pane titles are present + mouse hit-test maps columns to the
//! right pane. The center defaults to the primary CHAT; the right column is
//! agents (top) + the `.mf` passport viewer (bottom).

use kei_tui::app::App;
use kei_tui::types::Pane;
use kei_tui::ui::{draw, pane_at};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

#[test]
fn draws_three_panes_without_panic() {
    let backend = TestBackend::new(100, 30);
    let mut term = Terminal::new(backend).unwrap();
    let mut app = App::new(std::env::temp_dir()).expect("init app");
    // Sidebars are hidden by default; show the tree and PIN both right-column
    // panels so their titles render (the right sidebar shows only pinned panes).
    app.tree_collapsed = false;
    app.right_collapsed = false;
    app.pin_agents = true;
    app.pin_passport = true;
    term.draw(|f| draw(f, &mut app)).unwrap();

    let buf = term.backend().buffer().clone();
    let screen: String = buf.content().iter().map(|c| c.symbol()).collect();
    assert!(screen.contains("files"), "file-tree pane title missing");
    assert!(screen.contains("chat"), "primary chat pane title missing");
    assert!(screen.contains("agents"), "agents pane title missing");
    assert!(screen.contains("passport"), "passport pane title missing");
}

#[test]
fn mouse_hit_test_maps_columns_to_panes() {
    let area = Rect::new(0, 0, 100, 30);
    // No sticky top header — content columns start at row 0 (the chat is the page).
    assert_eq!(pane_at(area, 5, 10, false, false), Some(Pane::Tree)); // left ~28%
    assert_eq!(pane_at(area, 50, 10, false, false), Some(Pane::Terminal)); // center ~46%
    assert_eq!(pane_at(area, 90, 10, false, false), Some(Pane::Agents)); // right ~26%
}

#[test]
fn collapsing_a_side_widens_the_center() {
    use kei_tui::ui::regions;
    let area = Rect::new(0, 0, 100, 30);
    let [_, _, center_open, right_open, _, _] = regions(area, false, false);
    let [_, _, center_rc, right_rc, _, _] = regions(area, false, true); // right collapsed
    assert!(right_rc.width < right_open.width, "collapsed right is narrower");
    assert!(center_rc.width > center_open.width, "center widens when right collapses");
    assert_eq!(right_rc.width, 0, "a collapsed side is hidden entirely (width 0)");
}
