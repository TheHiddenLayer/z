//! Agent table widget — the home-screen list of agents with status, MR
//! state, branch (with drift arrow), base, and repo.
//!
//! Selection and scroll are passed in by reference; the widget is otherwise
//! stateless. Callers pre-compute MR display kinds aligned with `agents` so
//! this widget stays decoupled from `App` and snapshot resolution.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Widget},
};

use crate::agent::{Agent, AgentStatus};
use crate::gitlab::MrDisplayKind;
use crate::style::{DIM, TEXT, drift_arrow, status_color};

const SPINNER_FRAMES: [&str; 10] = [
    "\u{280B}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283C}", "\u{2834}", "\u{2826}", "\u{2827}",
    "\u{2807}", "\u{280F}",
];

/// Builder-lite widget. Construct via [`new`], chain optional setters, then
/// `frame.render_widget(&widget, area)`.
pub struct AgentTableWidget<'a> {
    agents: &'a [Agent],
    /// MR display kind for each agent, aligned 1:1 with `agents`. Empty slice
    /// is treated as "no MR data" for every row.
    mr_kinds: &'a [MrDisplayKind],
    selected: usize,
    spinner_frame: usize,
    empty_message: &'a str,
}

impl<'a> AgentTableWidget<'a> {
    pub fn new(agents: &'a [Agent]) -> Self {
        Self {
            agents,
            mr_kinds: &[],
            selected: 0,
            spinner_frame: 0,
            empty_message: "No agents yet.",
        }
    }

    pub fn mr_kinds(mut self, kinds: &'a [MrDisplayKind]) -> Self {
        self.mr_kinds = kinds;
        self
    }

    pub fn selected(mut self, index: usize) -> Self {
        self.selected = index;
        self
    }

    pub fn spinner_frame(mut self, frame: usize) -> Self {
        self.spinner_frame = frame;
        self
    }

    pub fn empty_message(mut self, msg: &'a str) -> Self {
        self.empty_message = msg;
        self
    }
}

impl Widget for &AgentTableWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.agents.is_empty() {
            let line = Line::from(Span::styled(
                self.empty_message,
                Style::default().fg(DIM),
            ));
            Paragraph::new(line).render(area, buf);
            return;
        }

        let visible_rows = (area.height as usize).saturating_sub(2);
        let offset = if visible_rows == 0 {
            0
        } else if self.selected >= visible_rows {
            self.selected - visible_rows + 1
        } else {
            0
        };

        let repo_w = self
            .agents
            .iter()
            .map(|a| a.repo_name.len())
            .max()
            .unwrap_or(0)
            .max(4) as u16;

        let branch_w = self
            .agents
            .iter()
            .map(|a| {
                if a.slug != a.branch.replace('/', "-") {
                    a.slug.len() + 3 + a.branch.len()
                } else {
                    a.branch.len()
                }
            })
            .max()
            .unwrap_or(0)
            .max(6) as u16;

        let has_base = self
            .agents
            .iter()
            .any(|a| a.base_branch.as_deref().is_some_and(|b| !b.is_empty()));
        let base_col_w = if has_base {
            self.agents
                .iter()
                .map(|a| a.base_branch.as_deref().unwrap_or("").len())
                .max()
                .unwrap_or(0)
                .max(4) as u16
        } else {
            0
        };
        let status_w: u16 = 1;
        let mr_w: u16 = 7;

        let mut rows: Vec<Row> = Vec::new();
        for (i, agent) in self
            .agents
            .iter()
            .enumerate()
            .skip(offset)
            .take(visible_rows)
        {
            let is_selected = i == self.selected;
            let indicator = if is_selected { "\u{2502}" } else { " " };
            let indicator_style = if is_selected {
                Style::default().fg(TEXT)
            } else {
                Style::default()
            };
            let text_style = if is_selected {
                Style::default().fg(TEXT)
            } else {
                Style::default().fg(DIM)
            };

            let base_cell = match agent.base_branch.as_deref() {
                Some(b) if !b.is_empty() => Line::from(Span::styled(b.to_string(), text_style)),
                _ => Line::from(""),
            };

            let drifted = agent.slug != agent.branch.replace('/', "-");
            let branch_cell = if drifted {
                Line::from(vec![
                    Span::styled(agent.slug.clone(), text_style),
                    drift_arrow(),
                    Span::styled(
                        agent.branch.clone(),
                        text_style.add_modifier(Modifier::ITALIC),
                    ),
                ])
            } else {
                Line::from(Span::styled(agent.branch.clone(), text_style))
            };

            let mr_kind = self.mr_kinds.get(i).copied().unwrap_or(MrDisplayKind::None);

            rows.push(Row::new(vec![
                Cell::from(Span::styled(indicator, indicator_style)),
                Cell::from(status_glyph(agent, self.spinner_frame)),
                Cell::from(Span::styled(mr_status_label(mr_kind), text_style)),
                Cell::from(branch_cell),
                Cell::from(base_cell),
                Cell::from(Span::styled(agent.repo_name.clone(), text_style)),
            ]));
        }

        let hdr_style = Style::default().fg(DIM);
        let header = Row::new(vec![
            Cell::from(""),
            Cell::from(""),
            Cell::from(Span::styled("MR", hdr_style)),
            Cell::from(Span::styled("BRANCH", hdr_style)),
            Cell::from(Span::styled("BASE", hdr_style)),
            Cell::from(Span::styled("REPO", hdr_style)),
        ])
        .bottom_margin(1);

        let table = Table::new(
            rows,
            [
                Constraint::Length(1),
                Constraint::Length(status_w + 1),
                Constraint::Length(mr_w + 1),
                Constraint::Length(branch_w + 2),
                Constraint::Length(if base_col_w > 0 { base_col_w + 2 } else { 0 }),
                Constraint::Min(repo_w),
            ],
        )
        .header(header)
        .block(Block::default().borders(Borders::NONE));

        Widget::render(table, area, buf);
    }
}

fn status_glyph(agent: &Agent, frame_idx: usize) -> Span<'static> {
    let style = Style::default().fg(status_color(agent));
    match &agent.status {
        AgentStatus::Error(_) => Span::styled("\u{2717}", style),
        AgentStatus::Stopped => Span::styled("\u{2212}", style),
        _ if agent.shows_spinner() => {
            Span::styled(SPINNER_FRAMES[frame_idx % SPINNER_FRAMES.len()], style)
        }
        _ => Span::styled("\u{2713}", style),
    }
}

fn mr_status_label(kind: MrDisplayKind) -> &'static str {
    match kind {
        MrDisplayKind::None => "",
        MrDisplayKind::Unknown => "unknown",
        MrDisplayKind::Draft => "draft",
        MrDisplayKind::Ready => "ready",
        MrDisplayKind::Blocked => "blocked",
        MrDisplayKind::Open => "open",
        MrDisplayKind::Merged => "merged",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use crate::agent::tests::make_agent_with_status;

    fn render_widget(widget: &AgentTableWidget<'_>, width: u16, height: u16) -> Terminal<TestBackend> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| f.render_widget(widget, f.area()))
            .unwrap();
        terminal
    }

    #[test]
    fn empty_agents_renders_empty_message() {
        let widget = AgentTableWidget::new(&[]).empty_message("No agents yet.");
        let terminal = render_widget(&widget, 40, 4);
        let buf = terminal.backend().buffer();
        let first_line: String = (0..buf.area.width)
            .map(|x| buf[(x, 0)].symbol())
            .collect::<Vec<_>>()
            .join("");
        assert!(
            first_line.contains("No agents yet."),
            "missing empty message in {first_line:?}"
        );
    }

    fn buffer_to_string(terminal: &Terminal<TestBackend>) -> String {
        let buf = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn renders_agent_row_with_branch_and_repo() {
        let mut agent = make_agent_with_status(AgentStatus::Running);
        agent.repo_name = "myrepo".into();
        agent.branch = "feature/x".into();
        agent.slug = "feature-x".into();

        let agents = vec![agent];
        let widget = AgentTableWidget::new(&agents).selected(0);
        let terminal = render_widget(&widget, 60, 6);
        let dump = buffer_to_string(&terminal);
        assert!(dump.contains("feature/x"), "branch missing in:\n{dump}");
        assert!(dump.contains("myrepo"), "repo missing in:\n{dump}");
    }

    #[test]
    fn mr_kind_renders_label_when_provided() {
        let agent = make_agent_with_status(AgentStatus::Running);
        let agents = vec![agent];
        let kinds = vec![MrDisplayKind::Ready];
        let widget = AgentTableWidget::new(&agents).mr_kinds(&kinds);
        let terminal = render_widget(&widget, 60, 6);
        let dump = buffer_to_string(&terminal);
        assert!(dump.contains("ready"), "MR label missing in:\n{dump}");
    }
}
