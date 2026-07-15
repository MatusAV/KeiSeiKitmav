//! Dev aid: render one cockpit frame to a headless TestBackend and print it
//! as text (no tty needed). `cargo run -p kei-tui --example dump_frame`.

use kei_tui::app::App;
use kei_tui::ui::draw;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn main() {
    let (w, h) = (86u16, 20u16);
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut app = App::new(std::env::current_dir().unwrap()).expect("init app");
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
