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
    widgets::{Block, Borders, Cell, Paragraph, Row, StatefulWidget, Table, TableState, Widget},
};

use crate::agent::Agent;
use crate::gitlab::MrDisplayKind;
use crate::style::{DIM, TEXT, drift_arrow, status_color};

fn table_scroll_offset(selected: usize, visible_rows: usize) -> usize {
    if visible_rows == 0 {
        0
    } else {
        selected.saturating_add(1).saturating_sub(visible_rows)
    }
}

/// Builder-lite widget. Construct via [`Self::new`], chain optional setters, then
/// `frame.render_widget(&widget, area)`.
pub struct AgentTableWidget<'a> {
    agents: &'a [Agent],
    /// MR display kind for each agent, aligned 1:1 with `agents`. Empty slice
    /// is treated as "no MR data" for every row.
    mr_kinds: &'a [MrDisplayKind],
    selected: usize,
    empty_message: &'a str,
}

impl<'a> AgentTableWidget<'a> {
    pub fn new(agents: &'a [Agent]) -> Self {
        Self {
            agents,
            mr_kinds: &[],
            selected: 0,
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

    pub fn empty_message(mut self, msg: &'a str) -> Self {
        self.empty_message = msg;
        self
    }
}

impl Widget for &AgentTableWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.agents.is_empty() {
            let line = Line::from(Span::styled(self.empty_message, Style::default().fg(DIM)));
            Paragraph::new(line).render(area, buf);
            return;
        }

        let visible_rows = (area.height as usize).saturating_sub(2);
        let selected = self.selected.min(self.agents.len().saturating_sub(1));
        let offset = table_scroll_offset(selected, visible_rows);

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
        for (i, agent) in self.agents.iter().enumerate() {
            let is_selected = i == selected;
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
                Cell::from(status_dot(agent)),
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

        let mut table_state = TableState::default()
            .with_selected(Some(selected))
            .with_offset(offset);
        StatefulWidget::render(table, area, buf, &mut table_state);
    }
}

fn status_dot(agent: &Agent) -> Span<'static> {
    Span::styled("\u{25CF}", Style::default().fg(status_color(agent)))
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
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::agent::{AgentStatus, tests::make_agent_with_status};

    fn render_widget(
        widget: &AgentTableWidget<'_>,
        width: u16,
        height: u16,
    ) -> Terminal<TestBackend> {
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

    #[test]
    fn status_column_renders_single_colored_dot_for_every_state() {
        fn rendered_status_cell(agent: Agent) -> (String, ratatui::style::Color) {
            let agents = vec![agent];
            let widget = AgentTableWidget::new(&agents);
            let terminal = render_widget(&widget, 40, 4);
            let cell = &terminal.backend().buffer()[(2, 2)];
            (cell.symbol().to_string(), cell.fg)
        }

        let mut active = make_agent_with_status(AgentStatus::Running);
        active.seen_activity_since_seed = true;
        active.last_pane_hash = Some(1);

        let agents = [
            active,
            make_agent_with_status(AgentStatus::Running),
            make_agent_with_status(AgentStatus::Stopped),
            make_agent_with_status(AgentStatus::Error("boom".into())),
        ];

        for agent in agents {
            let expected_color = status_color(&agent);
            let (symbol, color) = rendered_status_cell(agent);
            assert_eq!(symbol, "\u{25CF}");
            assert_eq!(color, expected_color);
        }
    }

    #[test]
    fn table_scroll_offset_keeps_selected_last_visible() {
        assert_eq!(table_scroll_offset(0, 4), 0);
        assert_eq!(table_scroll_offset(3, 4), 0);
        assert_eq!(table_scroll_offset(4, 4), 1);
        assert_eq!(table_scroll_offset(7, 4), 4);
        assert_eq!(table_scroll_offset(7, 0), 0);
    }

    #[test]
    fn selected_agent_near_end_is_visible() {
        let mut agents = Vec::new();
        for n in 1..=8 {
            let mut agent = make_agent_with_status(AgentStatus::Running);
            agent.repo_name = "myrepo".into();
            agent.branch = format!("branch-{n}");
            agent.slug = format!("branch-{n}");
            agents.push(agent);
        }

        let widget = AgentTableWidget::new(&agents).selected(7);
        let terminal = render_widget(&widget, 60, 6);
        let dump = buffer_to_string(&terminal);

        assert!(
            dump.contains("branch-8"),
            "selected branch missing in:\n{dump}"
        );
        assert!(
            !dump.contains("branch-1"),
            "first branch should scroll out:\n{dump}"
        );
    }

    #[test]
    fn overflowing_table_does_not_render_scrollbar() {
        let mut agents = Vec::new();
        for n in 1..=8 {
            let mut agent = make_agent_with_status(AgentStatus::Running);
            agent.repo_name = "myrepo".into();
            agent.branch = format!("branch-{n}");
            agent.slug = format!("branch-{n}");
            agents.push(agent);
        }

        let widget = AgentTableWidget::new(&agents).selected(7);
        let terminal = render_widget(&widget, 80, 6);
        let buf = terminal.backend().buffer();
        let right_edge_x = buf.area.width.saturating_sub(1);
        let right_edge: Vec<&str> = (0..buf.area.height)
            .map(|y| buf[(right_edge_x, y)].symbol())
            .collect();

        assert!(
            right_edge.iter().all(|symbol| *symbol == " "),
            "overflowing bottom agent table should not render a scrollbar on the right edge:\n{}",
            buffer_to_string(&terminal)
        );
    }
}
