//! Global key actions available from ANY pane — including while the embedded
//! shell has focus — so the user can always cycle focus, launch an agent, or
//! quit without the shell swallowing the key. Pane-local keys (tree/agents
//! navigation, shell input) are routed by `runner` based on the focused pane.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A global action, or `None` to let the focused pane handle the key.
#[derive(Debug, PartialEq, Eq)]
pub enum Global {
    Quit,
    FocusNext,
    Launch,
    Theme,
}

/// Map a key to a global action. Uses F-keys + Ctrl-Q so they never collide
/// with characters typed into the shell.
pub fn global(k: KeyEvent) -> Option<Global> {
    match k.code {
        KeyCode::Char('q') if k.modifiers.contains(KeyModifiers::CONTROL) => Some(Global::Quit),
        KeyCode::F(2) => Some(Global::FocusNext), // cycle focus (works even in the terminal)
        KeyCode::F(3) => Some(Global::Theme),     // cycle theme (KeiLab dark/light/default)
        KeyCode::F(5) => Some(Global::Launch),    // launch a GLM agent via our runtime
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn ev(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent { code, modifiers: m, kind: KeyEventKind::Press, state: KeyEventState::NONE }
    }

    #[test]
    fn globals_map_correctly_and_plain_keys_pass_through() {
        assert_eq!(global(ev(KeyCode::Char('q'), KeyModifiers::CONTROL)), Some(Global::Quit));
        assert_eq!(global(ev(KeyCode::F(2), KeyModifiers::NONE)), Some(Global::FocusNext));
        assert_eq!(global(ev(KeyCode::F(5), KeyModifiers::NONE)), Some(Global::Launch));
        // a plain char is NOT global > goes to the focused pane (e.g. the shell)
        assert_eq!(global(ev(KeyCode::Char('q'), KeyModifiers::NONE)), None);
        assert_eq!(global(ev(KeyCode::Char('a'), KeyModifiers::NONE)), None);
    }
}
