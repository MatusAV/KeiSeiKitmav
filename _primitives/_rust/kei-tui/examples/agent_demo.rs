//! Live proof of OUR agent system in the cockpit sidebar (Path A — GLM via
//! kei-cortex, no Claude). Launches a real agent run against $KEI_TUI_BASE
//! (default :9800), pumps its events into the App, and prints the final frame
//! so the agents pane shows the run's status + tool + token count.
//!
//! Run:  KEI_TUI_BASE=http://127.0.0.1:9800 cargo run -p kei-tui --example agent_demo

use std::time::Duration;

use kei_tui::app::App;
use kei_tui::runs::{spawn_run, RunConfig, RunEvent};
use kei_tui::ui::draw;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let mut app = App::new(std::env::current_dir().unwrap()).expect("init cockpit");
    let (tx, mut rx) = mpsc::unbounded_channel::<RunEvent>();
    let cfg = RunConfig::from_env();
    println!(
        "launching GLM agent via {} (provider={}, model={}) — 0 Claude binary\n",
        cfg.base, cfg.provider, cfg.model
    );
    spawn_run(
        cfg,
        "List the files and directories in the current working directory using your tools, \
         then reply with how many there are."
            .to_string(),
        "glm-agent-1".to_string(),
        "generalist".to_string(),
        "list files + count".to_string(),
        tx.clone(),
    );

    let pump = async {
        while let Some(ev) = rx.recv().await {
            let end = matches!(ev, RunEvent::Done { .. } | RunEvent::Error { .. });
            if let RunEvent::Tool { name, phase, .. } = &ev {
                println!("  tool: {name} {phase}");
            }
            app.apply_run_event(ev);
            if end {
                break;
            }
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(60), pump).await;

    println!("\n--- cockpit frame (agents pane populated by OUR runtime) ---");
    let (w, h) = (86u16, 12u16);
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
