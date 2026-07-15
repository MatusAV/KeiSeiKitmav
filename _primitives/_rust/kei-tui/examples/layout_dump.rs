//! Frame dump — border/alignment/spinner eyeball gate.
//!   (no arg) clean default (sidebars hidden)   |  `busy` oracle spinner
//!   `agent`  agent full-screen + session strip  |  `panes` sidebars shown
use kei_tui::app::{App, CenterMode};
use kei_tui::agents::{AgentCard, AgentStatus};
use kei_tui::chat::Role;
use kei_tui::ui::draw;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use std::time::Instant;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let (w, h) = (96u16, 22u16);
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut app = App::new(std::env::temp_dir()).expect("init app");
    app.chat.msgs.push(kei_tui::chat::Msg { role: Role::User, text: "есть план?".into(), image: None });
    app.chat.msgs.push(kei_tui::chat::Msg { role: Role::Agent, text: "да — вот он.".into(), image: None });
    match mode.as_str() {
        "busy" => { app.oracle_busy = true; app.oracle_started = Instant::now(); }
        "panes" => { app.tree_collapsed = false; app.right_collapsed = false; }
        "agent" => {
            app.tree_collapsed = false; app.right_collapsed = false;
            app.agents.cards.push(AgentCard { id: "a1".into(), label: "glm-agent-1".into(),
                role: "generalist".into(), task: "list files".into(), status: AgentStatus::Running,
                last_tool: Some("bash".into()), tokens: 42, started: Instant::now(),
                log: vec!["● started".into()] });
            app.center = CenterMode::Agent("a1".into());
        }
        _ => {}
    }
    term.draw(|f| draw(f, &mut app)).unwrap();
    let buf = term.backend().buffer().clone();
    for y in 0..h { println!("{}", (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>()); }
}
