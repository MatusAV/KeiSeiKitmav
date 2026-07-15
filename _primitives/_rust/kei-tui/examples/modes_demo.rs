//! Headless proof that the center-pane modes (t20-t22) each render through
//! `ui::draw`: editor (t21), 2/5·3/5 chat split (t20/t24), settings (t22).
//! Dumps a signature line from each so the wiring is verifiable without a TTY.

use kei_tui::app::{App, CenterMode};
use kei_tui::chat::Role;
use kei_tui::runs::RunEvent;
use kei_tui::ui::draw;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn dump(app: &mut App, tag: &str) {
    let mut term = Terminal::new(TestBackend::new(110, 22)).unwrap();
    term.draw(|f| draw(f, app)).unwrap();
    let buf = term.backend().buffer().clone();
    // Flatten every row to text and print the first few non-blank lines.
    let mut shown = 0;
    println!("──────── {tag} ────────");
    for y in 0..buf.area.height {
        let mut line = String::new();
        for x in 0..buf.area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        let t = line.trim_end();
        if !t.trim().is_empty() {
            println!("{t}");
            shown += 1;
            if shown >= 6 {
                break;
            }
        }
    }
    println!();
}

fn main() {
    let mut app = App::new(std::env::current_dir().unwrap()).unwrap();

    // Chat is primary: a live agent's TEXT mirrors into the chat; tool actions
    // stay on the sidebar card (t20/t24).
    app.center = CenterMode::Chat;
    app.apply_run_event(RunEvent::Started {
        id: "r1".into(),
        label: "glm-agent-1".into(),
        role: "generalist".into(),
        task: "write a mini landing page".into(),
    });
    app.apply_run_event(RunEvent::Tool { id: "r1".into(), name: "write".into(), phase: "start".into() , resource: None, added: None, removed: None });
    app.apply_run_event(RunEvent::Delta { id: "r1".into(), text: "creating index.html".into() });
    app.chat.push(Role::User, "make the hero teal".into());
    dump(&mut app, "CHAT (primary, full center)");

    // Editor rides ABOVE the chat when a file is open (t21).
    app.editor.open(std::path::PathBuf::from("examples/modes_demo.rs"));
    dump(&mut app, "EDITOR ABOVE CHAT (t21)");

    // t22 — settings panel.
    app.center = CenterMode::Settings;
    dump(&mut app, "SETTINGS (t22)");
}
