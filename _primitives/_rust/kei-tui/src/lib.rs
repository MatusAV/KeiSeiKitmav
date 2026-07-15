//! `kei-tui` — native TUI cockpit for the kei-cortex agent runtime.
//!
//! Constructor pattern (skeleton seeded from `kei-tty`):
//!   * [`types`]  — `Pane` enum + focus cycle order.
//!   * [`app`]    — cockpit state machine (focus + per-pane content).
//!   * [`keys`]   — crossterm `KeyEvent` > state transition.
//!   * [`ui`]     — ratatui rendering + shared pane layout (`regions`/`pane_at`).
//!   * [`runner`] — async event loop (keys + mouse).
//!
//! t00 scaffold: three focusable panes (files | terminal | agents) + Tab/mouse
//! focus. Panes are placeholders until t01 (lazy file tree), t02 (embedded PTY
//! via tui-term), t03 (live `/v1/runs` agent mini-windows).

pub mod agents;
pub mod app;
pub mod chat;
pub mod editor;
pub mod header;
pub mod keys;
pub mod day_total;
pub mod dna;
pub mod image_pane;
pub mod legend;
pub mod mf_view;
pub mod palette;
pub mod settings;
pub mod runner;
pub mod runs;
pub mod session;
pub mod splash;
pub mod sphere;
pub mod term;
pub mod theme;
pub mod tokens;
pub mod tree;
pub mod types;
pub mod voice;
pub mod ui;
