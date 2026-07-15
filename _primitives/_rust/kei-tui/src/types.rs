//! Shared cockpit types: the three focusable panes + focus cycle order.

/// The three focusable panes of the cockpit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    /// Left: lazy file tree (t01).
    Tree,
    /// Center: embedded shell PTY (t02).
    Terminal,
    /// Right: live current-session agent mini-windows (t03).
    Agents,
}

impl Pane {
    /// Cycle order for Tab / BackTab focus movement.
    pub const ORDER: [Pane; 3] = [Pane::Tree, Pane::Terminal, Pane::Agents];

    /// Next pane in cycle order (wraps).
    pub fn next(self) -> Pane {
        let i = Self::ORDER.iter().position(|&p| p == self).unwrap_or(0);
        Self::ORDER[(i + 1) % Self::ORDER.len()]
    }

    /// Previous pane in cycle order (wraps).
    pub fn prev(self) -> Pane {
        let i = Self::ORDER.iter().position(|&p| p == self).unwrap_or(0);
        Self::ORDER[(i + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }

    /// Short label for the pane title / status bar.
    pub fn label(self) -> &'static str {
        match self {
            Pane::Tree => "files",
            Pane::Terminal => "terminal",
            Pane::Agents => "agents",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_cycles_forward_and_back() {
        assert_eq!(Pane::Tree.next(), Pane::Terminal);
        assert_eq!(Pane::Terminal.next(), Pane::Agents);
        assert_eq!(Pane::Agents.next(), Pane::Tree);
        assert_eq!(Pane::Tree.prev(), Pane::Agents);
        assert_eq!(Pane::Agents.prev(), Pane::Terminal);
    }
}
