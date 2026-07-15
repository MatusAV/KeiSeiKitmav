//! Headless proof of the agent DETAIL view (t12) + spinning Frobenius sphere
//! (t15): inject a running agent, open its detail, dump the center pane.

use kei_tui::app::{App, CenterMode};
use kei_tui::runs::RunEvent;
use kei_tui::types::Pane;
use kei_tui::ui::draw;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn main() {
    let mut app = App::new(std::env::current_dir().unwrap()).unwrap();
    app.apply_run_event(RunEvent::Started {
        id: "r1".into(),
        label: "glm-agent-1".into(),
        role: "generalist".into(),
        task: "list files in the current directory + count them".into(),
    });
    app.apply_run_event(RunEvent::Tool { id: "r1".into(), name: "bash".into(), phase: "start".into() , resource: None, added: None, removed: None });
    app.apply_run_event(RunEvent::Delta {
        id: "r1".into(),
        text: "Running ls -a in the project root to enumerate entries…".into(),
    });
    app.center = CenterMode::Agent("r1".into());
    app.focus = Pane::Agents;

    let (w, h) = (100u16, 20u16);
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| draw(f, &mut app)).unwrap();
    let buf = term.backend().buffer().clone();
    for y in 0..h {
        let mut line = String::new();
        for x in 0..w {
            line.push_str(buf[(x, y)].symbol());
        }
        println!("{}", line.trim_end());
    }
}
