//! Design tokens and render helpers for z's TUI.
//!
//! Six semantic colors. Status colors (OK/BUSY/FAIL) appear only on agent
//! status indicators — never in chrome. ACCENT means "current selection or
//! focus" — never status, never decoration.

use ratatui::style::Color;

use crate::agent::{Agent, AgentStatus};

pub const TEXT: Color = Color::Reset;
pub const DIM: Color = Color::DarkGray;
pub const ACCENT: Color = Color::Cyan;
pub const OK: Color = Color::Green;
pub const BUSY: Color = Color::Yellow;
pub const FAIL: Color = Color::Red;

pub fn status_color(agent: &Agent) -> Color {
    match &agent.status {
        AgentStatus::Error(_) => FAIL,
        AgentStatus::Stopped => DIM,
        _ if agent.shows_spinner() => BUSY,
        _ => OK,
    }
}

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

    use crate::agent::{AgentStatus, tests::make_agent_with_status};

    #[test]
    fn status_color_error_is_fail() {
        let a = make_agent_with_status(AgentStatus::Error("boom".into()));
        assert_eq!(status_color(&a), FAIL);
    }

    #[test]
    fn status_color_stopped_is_dim() {
        let a = make_agent_with_status(AgentStatus::Stopped);
        assert_eq!(status_color(&a), DIM);
    }

    #[test]
    fn status_color_creating_is_busy() {
        // shows_spinner() is unconditionally true for Creating (agent.rs:125).
        let a = make_agent_with_status(AgentStatus::Creating);
        assert_eq!(status_color(&a), BUSY);
    }

    #[test]
    fn status_color_idle_running_is_ok() {
        // Running + last_pane_hash=None + was_spinner_visible=false → idle
        // (see shows_spinner_follows_was_spinner_visible_when_hash_cleared in agent.rs).
        let mut a = make_agent_with_status(AgentStatus::Running);
        a.last_pane_hash = None;
        a.was_spinner_visible = false;
        assert!(!a.shows_spinner());
        assert_eq!(status_color(&a), OK);
    }
}
