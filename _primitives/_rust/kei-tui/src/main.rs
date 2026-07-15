//! `kei-tui` — native TUI cockpit for kei-cortex.
//!
//! Three live panes: a lazy file tree (left), an embedded shell PTY (center),
//! and a right sidebar of the agents launched THIS session — which run through
//! OUR kei-cortex runtime on GLM (Path A), never Claude Code. F5 launches a
//! demo agent; drag a file from the tree onto the terminal to insert its path.

use anyhow::{Context, Result};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use kei_tui::app::App;
use kei_tui::runner;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("kei-tui: {e:#}");
        std::process::exit(2);
    }
}

async fn run() -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let app = App::new(cwd).context("init cockpit")?;

    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).context("enter alt screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("init terminal")?;

    let res = runner::run(&mut terminal, app).await;

    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
    // Restore the user's own terminal colors (undo any theme OSC we pushed).
    use std::io::Write;
    let _ = write!(io::stdout(), "\x1b]110\x07\x1b]111\x07");
    let _ = io::stdout().flush();
    res
}
