//! Runtime-switchable palette. Three themes: KeiLab dark, KeiLab light, and
//! "terminal default" (respects the user's own terminal colors). Panes stay
//! transparent; the terminal's fg/bg is set per theme via OSC 10/11 escape
//! sequences (OSC 110/111 to RESET back to the user's colors for the default).
//!
//! Palette values come from `keilab-site/src/app/globals.css` +
//! `forgejo-keilab-theme`. Cycle with F3.

use ratatui::style::Color;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Copy)]
pub struct Palette {
    pub paper: Color,
    pub ink: Color,
    pub muted: Color,
    pub grid: Color,
    pub accent: Color,
    pub accent2: Color,
    pub done: Color,
    /// (fg_hex, bg_hex) to push to the host terminal via OSC; None = reset to
    /// the user's own terminal colors (the "standard" theme).
    pub term_hex: Option<(&'static str, &'static str)>,
}

impl Palette {
    /// The escape sequence that applies this palette's fg/bg to the host terminal.
    pub fn osc(&self) -> String {
        match self.term_hex {
            Some((fg, bg)) => format!("\x1b]10;{fg}\x07\x1b]11;{bg}\x07"),
            None => "\x1b]110\x07\x1b]111\x07".to_string(), // reset fg + bg
        }
    }
}

pub const KEILAB_DARK: Palette = Palette {
    paper: Color::Rgb(0x1F, 0x16, 0x12),
    ink: Color::Rgb(0xE8, 0xE5, 0xDC),
    muted: Color::Rgb(0x9A, 0x96, 0x8B),
    grid: Color::Rgb(0x4E, 0x44, 0x3A),
    accent: Color::Rgb(0xFF, 0x6B, 0x73),
    accent2: Color::Rgb(0x5C, 0xB0, 0xA8),
    done: Color::Rgb(0x7C, 0xC2, 0x8A),
    // Dark-chocolate background (#1F1612) — a touch warmer than the old near-black
    // #14161A, requested as the cockpit's base tone.
    term_hex: Some(("#E8E5DC", "#1F1612")),
};

pub const KEILAB_LIGHT: Palette = Palette {
    paper: Color::Rgb(0xF6, 0xF1, 0xE3),
    ink: Color::Rgb(0x1A, 0x1A, 0x1A),
    muted: Color::Rgb(0x5C, 0x55, 0x47),
    grid: Color::Rgb(0xC8, 0xC1, 0xA6),
    accent: Color::Rgb(0xCE, 0x3F, 0x38),
    accent2: Color::Rgb(0x26, 0x46, 0x53),
    done: Color::Rgb(0x2E, 0x7D, 0x46),
    term_hex: Some(("#1A1A1A", "#F6F1E3")),
};

/// "Standard" — keep the user's own terminal theme; only structural accents from
/// the terminal's ANSI palette, surfaces transparent.
pub const TERMINAL_DEFAULT: Palette = Palette {
    paper: Color::Reset,
    ink: Color::Reset,
    muted: Color::DarkGray,
    grid: Color::DarkGray,
    accent: Color::Cyan,
    accent2: Color::Green,
    done: Color::Green,
    term_hex: None,
};

pub const THEMES: [Palette; 3] = [KEILAB_DARK, KEILAB_LIGHT, TERMINAL_DEFAULT];
pub const THEME_NAMES: [&str; 3] = ["KeiLab dark", "KeiLab light", "terminal default"];

static IDX: AtomicUsize = AtomicUsize::new(0);

/// The active palette.
pub fn palette() -> Palette {
    THEMES[IDX.load(Ordering::Relaxed) % THEMES.len()]
}

/// Name of the active theme (for the status bar).
pub fn name() -> &'static str {
    THEME_NAMES[IDX.load(Ordering::Relaxed) % THEMES.len()]
}

/// Advance to the next theme; returns the now-active palette.
pub fn cycle() -> Palette {
    IDX.fetch_add(1, Ordering::Relaxed);
    palette()
}
