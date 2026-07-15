//! Inline images in the chat (t05) — render a PNG/JPEG in the terminal via
//! `ratatui-image` (kitty / iTerm2 / sixel graphics protocols, with a
//! unicode-halfblock fallback for terminals that support none).
//!
//! Host-terminal dependent: fancy protocols only render in a supporting terminal
//! (kitty/WezTerm/Ghostty/foot/iTerm2). Everywhere else — and in CI / TestBackend
//! / a non-interactive pipe — we fall back to halfblocks, which paint with
//! ordinary `▀` cells and always work. Protocol detection (`from_query_stdio`)
//! sends an escape query and waits for a reply; under a pipe it would hang, so we
//! only attempt it when stdout is a TTY and fall back otherwise.

use ratatui::layout::Rect;
use ratatui::Frame;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, StatefulImage};
use std::collections::HashMap;

/// Holds the terminal-protocol picker + a cache of decoded image protocols,
/// keyed by a small integer id the chat assigns to each inline image.
pub struct ImagePane {
    picker: Picker,
    cache: HashMap<usize, StatefulProtocol>,
    next_id: usize,
}

impl Default for ImagePane {
    fn default() -> Self {
        Self::new()
    }
}

impl ImagePane {
    /// Build with the best protocol the host terminal supports, falling back to
    /// halfblocks. `from_query_stdio` is only safe on a real TTY (it round-trips
    /// an escape sequence); under a pipe / test we go straight to halfblocks so
    /// nothing hangs.
    pub fn new() -> Self {
        let picker = detect_picker();
        Self { picker, cache: HashMap::new(), next_id: 0 }
    }

    /// Decode `bytes` (PNG/JPEG) into a cached protocol and return its id, or
    /// `None` if the bytes aren't a decodable image. The id is what the chat
    /// stores on the message and passes back to `render`.
    pub fn load(&mut self, bytes: &[u8]) -> Option<usize> {
        let img = image::load_from_memory(bytes).ok()?;
        let proto = self.picker.new_resize_protocol(img);
        let id = self.next_id;
        self.next_id += 1;
        self.cache.insert(id, proto);
        Some(id)
    }

    /// True once `load` has cached an image under `id`.
    pub fn has(&self, id: usize) -> bool {
        self.cache.contains_key(&id)
    }

    /// Render the image `id` into `area` (fit within it). No-op if `id` isn't
    /// cached or the area is empty.
    pub fn render(&mut self, f: &mut Frame, area: Rect, id: usize) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        if let Some(proto) = self.cache.get_mut(&id) {
            let widget = StatefulImage::<StatefulProtocol>::default().resize(Resize::Fit(None));
            f.render_stateful_widget(widget, area, proto);
        }
    }
}

/// A conservative default font cell size (w×h in pixels) for the halfblock
/// fallback when we can't query the real terminal. 8×16 is a common 1:2 cell.
const FALLBACK_FONT: (u16, u16) = (8, 16);

/// Pick a rendering protocol: query the terminal only when stdout is a TTY
/// (otherwise the query would block on a pipe); fall back to a fixed-font-size
/// picker (halfblocks — needs no terminal graphics support).
fn detect_picker() -> Picker {
    use std::io::IsTerminal;
    if std::io::stdout().is_terminal() {
        Picker::from_query_stdio().unwrap_or_else(|_| Picker::from_fontsize(FALLBACK_FONT))
    } else {
        Picker::from_fontsize(FALLBACK_FONT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// A 2×2 red PNG, base64-free (built at runtime), so the test needs no
    /// fixture file.
    fn tiny_png() -> Vec<u8> {
        let mut img = image::RgbaImage::new(2, 2);
        for p in img.pixels_mut() {
            *p = image::Rgba([200, 40, 40, 255]);
        }
        let dynimg = image::DynamicImage::ImageRgba8(img);
        let mut buf = std::io::Cursor::new(Vec::new());
        dynimg.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    #[test]
    fn detect_picker_falls_back_under_a_pipe_without_hanging() {
        // In `cargo test` stdout is not a TTY → the fixed-font-size fallback,
        // which must not hang (no terminal query) or panic.
        let _picker = detect_picker();
    }

    #[test]
    fn load_returns_an_id_for_a_valid_png_and_none_for_garbage() {
        let mut pane = ImagePane::new();
        let id = pane.load(&tiny_png());
        assert!(id.is_some(), "a valid PNG loads");
        assert!(pane.has(id.unwrap()));
        assert!(pane.load(b"not an image").is_none(), "garbage returns None");
    }

    #[test]
    fn render_does_not_panic_on_a_tiny_area() {
        let mut pane = ImagePane::new();
        let id = pane.load(&tiny_png()).unwrap();
        let mut term = Terminal::new(TestBackend::new(4, 3)).unwrap();
        term.draw(|f| pane.render(f, Rect::new(0, 0, 1, 1), id)).unwrap();
        // A missing id is a no-op, also no panic.
        term.draw(|f| pane.render(f, Rect::new(0, 0, 4, 3), 999)).unwrap();
    }
}
