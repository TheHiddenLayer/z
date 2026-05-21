//! Design tokens and render helpers for z's TUI.
//!
//! Five semantic colors. Status colors (OK/BUSY/FAIL) appear only on agent
//! status indicators — never in chrome. Selection and focus are encoded
//! monochromatically: brightness contrast (TEXT vs DIM) plus structural
//! glyphs like the `│` left bar. Color is reserved for status meaning.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::agent::{Agent, AgentStatus};

pub const TEXT: Color = Color::Reset;
pub const DIM: Color = Color::DarkGray;
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

/// Build a footer hint line: bold key + dim label, repeated, separated by ` · `.
///
/// One contract for every screen: keys are bold and at terminal-text brightness,
/// labels are dim, separator is a middle dot. Pairs are `(key, label)` so authors
/// keep the binding glyph and its verb visually adjacent in source.
pub fn footer_hint(items: &[(&str, &str)]) -> Line<'static> {
    let key_style = Style::default().fg(TEXT).add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(DIM);
    let sep = " \u{00b7} ";
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, (key, label)) in items.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(sep, label_style));
        }
        spans.push(Span::styled((*key).to_string(), key_style));
        spans.push(Span::styled(format!(" {label}"), label_style));
    }
    Line::from(spans)
}

/// The "→" between an agent's slug and its actual branch name when they
/// disagree. Drift is a structural fact, not a status — DIM, glyph carries
/// the meaning.
pub fn drift_arrow() -> Span<'static> {
    Span::styled(" \u{2192} ", Style::default().fg(DIM))
}

/// Modal/dialog title span. Bold + TEXT — emphasis from weight, not color.
pub fn modal_title(text: &str) -> Span<'static> {
    Span::styled(
        format!(" {text} "),
        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
    )
}

pub fn text_width(s: &str) -> usize {
    Span::raw(s).width()
}

fn take_prefix_width(s: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut width = 0;
    for ch in s.chars() {
        let ch_width = text_width(&ch.to_string());
        if width + ch_width > max_width {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out
}

fn take_suffix_width(s: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut width = 0;
    for ch in s.chars().rev() {
        let ch_width = text_width(&ch.to_string());
        if width + ch_width > max_width {
            break;
        }
        out.insert(0, ch);
        width += ch_width;
    }
    out
}

pub fn truncate_end(s: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if text_width(s) <= max_width {
        return s.to_string();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }
    let prefix = take_prefix_width(s, max_width - 3);
    format!("{prefix}...")
}

pub fn truncate_middle(s: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if text_width(s) <= max_width {
        return s.to_string();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }
    let available = max_width - 3;
    let prefix_width = available / 2 + available % 2;
    let suffix_width = available / 2;
    let prefix = take_prefix_width(s, prefix_width);
    let suffix = take_suffix_width(s, suffix_width);
    format!("{prefix}...{suffix}")
}

pub fn wrapped_line_count(text: &str, max_width: u16) -> u16 {
    let width = max_width.max(1) as usize;
    let count: usize = text
        .split('\n')
        .map(|line| text_width(line).max(1).div_ceil(width))
        .sum();
    count.min(u16::MAX as usize) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn tokens_have_expected_colors() {
        assert_eq!(TEXT, Color::Reset);
        assert_eq!(DIM, Color::DarkGray);
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

    use ratatui::style::Modifier;

    #[test]
    fn footer_hint_renders_single_pair() {
        let line = footer_hint(&[("q", "quit")]);
        // 2 spans: bold key, dim label
        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].content, "q");
        assert_eq!(
            line.spans[0].style,
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        );
        assert_eq!(line.spans[1].content, " quit");
        assert_eq!(line.spans[1].style, Style::default().fg(DIM));
    }

    #[test]
    fn footer_hint_inserts_bullet_separator_between_pairs() {
        let line = footer_hint(&[("q", "quit"), ("?", "help")]);
        // 5 spans: key, label, sep, key, label
        assert_eq!(line.spans.len(), 5);
        assert_eq!(line.spans[2].content, " \u{00b7} ");
        assert_eq!(line.spans[2].style, Style::default().fg(DIM));
    }

    #[test]
    fn footer_hint_no_trailing_separator() {
        let line = footer_hint(&[("a", "b"), ("c", "d")]);
        let last = line.spans.last().unwrap();
        assert_eq!(last.content, " d");
    }

    #[test]
    fn footer_hint_empty_input_yields_empty_line() {
        let line = footer_hint(&[]);
        assert!(line.spans.is_empty());
    }

    #[test]
    fn drift_arrow_is_dim_not_busy() {
        let span = drift_arrow();
        assert_eq!(span.content, " \u{2192} ");
        assert_eq!(span.style, Style::default().fg(DIM));
        assert_ne!(span.style.fg, Some(BUSY));
    }

    #[test]
    fn modal_title_is_bold_text() {
        let span = modal_title("New Agent");
        assert_eq!(span.content, " New Agent ");
        assert_eq!(
            span.style,
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        );
    }

    #[test]
    fn truncate_end_respects_visual_width() {
        assert_eq!(truncate_end("abcdef", 4), "a...");
        assert_eq!(truncate_end("界abcdef", 5), "界...");
        assert_eq!(truncate_end("abcdef", 2), "..");
    }

    #[test]
    fn truncate_middle_keeps_both_ends() {
        assert_eq!(truncate_middle("feature/render-table", 12), "featu...able");
        assert_eq!(truncate_middle("abcdef", 0), "");
    }

    #[test]
    fn wrapped_line_count_uses_display_width() {
        assert_eq!(wrapped_line_count("", 10), 1);
        assert_eq!(wrapped_line_count("abcd", 2), 2);
        assert_eq!(wrapped_line_count("界界", 2), 2);
        assert_eq!(wrapped_line_count("a\nbc", 10), 2);
    }
}
