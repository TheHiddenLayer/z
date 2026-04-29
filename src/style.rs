//! Design tokens and render helpers for z's TUI.
//!
//! Six semantic colors. Status colors (OK/BUSY/FAIL) appear only on agent
//! status indicators — never in chrome. ACCENT means "current selection or
//! focus" — never status, never decoration.

use ratatui::style::Color;

pub const TEXT: Color = Color::Reset;
pub const DIM: Color = Color::DarkGray;
pub const ACCENT: Color = Color::Cyan;
pub const OK: Color = Color::Green;
pub const BUSY: Color = Color::Yellow;
pub const FAIL: Color = Color::Red;

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn tokens_have_expected_colors() {
        assert_eq!(TEXT, Color::Reset);
        assert_eq!(DIM, Color::DarkGray);
        assert_eq!(ACCENT, Color::Cyan);
        assert_eq!(OK, Color::Green);
        assert_eq!(BUSY, Color::Yellow);
        assert_eq!(FAIL, Color::Red);
    }
}
