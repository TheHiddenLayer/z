use crate::app::{App, Mode, MrSnapshot, PreviewMode};
use crate::gitlab::{MergeRequest, MrDisplayKind, MrState, classify};
use crate::panel::NewAgentPanelWidget;
use crate::table::AgentTableWidget;
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

use crate::style::{
    BUSY, DIM, FAIL, OK, TEXT, drift_arrow, footer_hint, modal_title, status_color,
};

const AGENT_TABLE_HEIGHT: u16 = 6;
const BLANK_PANE_Z: &[&str] = &[
    "         ,----,",
    "       .'   .`|",
    "    .'   .'   ;",
    "  ,---, '    .'",
    "  |   :     ./",
    "  ;   | .'  /",
    "  `---' /  ;",
    "    /  ;  /",
    "   ;  /  /--,",
    "  /  /  / .`|",
    "./__;       :",
    "|   :     .'",
    ";   |  .'",
    "`---'",
];

pub fn draw(frame: &mut Frame, app: &App) {
    let inner = frame.area().inner(Margin {
        vertical: 1,
        horizontal: 3,
    });

    let layout = Layout::vertical([
        Constraint::Min(1),                     // preview pane
        Constraint::Length(1),                  // breathing room above separator
        Constraint::Length(1),                  // horizontal separator
        Constraint::Length(1),                  // breathing room below separator
        Constraint::Length(AGENT_TABLE_HEIGHT), // agent table: header + gap + 4 rows
        Constraint::Length(1),                  // breathing room above status bar
        Constraint::Length(1),                  // status bar
    ]);
    let [
        preview_pane,
        _above_separator,
        separator,
        _below_separator,
        agent_table,
        _above_status,
        status_bar,
    ] = inner.layout(&layout);

    if matches!(app.mode, Mode::NewAgent(_)) {
        let widget = NewAgentPanelWidget::new(app);
        frame.render_widget(&widget, preview_pane);
    } else {
        draw_preview(frame, app, preview_pane);
    }

    draw_separator(frame, app, separator);
    draw_agent_table(frame, app, agent_table);
    draw_status_bar(frame, app, status_bar);

    // Modal overlays
    match &app.mode {
        Mode::ConfirmDelete => {
            let modal_area = frame
                .area()
                .centered(Constraint::Percentage(52), Constraint::Percentage(28));
            frame.render_widget(Clear, modal_area);
            draw_delete_modal(frame, app, modal_area);
        }
        Mode::ConfirmMerge { .. } => {
            let modal_area = frame
                .area()
                .centered(Constraint::Percentage(54), Constraint::Percentage(26));
            frame.render_widget(Clear, modal_area);
            draw_merge_modal(frame, app, modal_area);
        }
        _ => {}
    }
}

fn mr_preview_lines(snapshot: Option<&MrSnapshot>) -> Vec<Line<'static>> {
    match snapshot {
        None | Some(MrSnapshot::Missing) => vec![
            Line::from(Span::styled("No merge request.", Style::default().fg(DIM))),
            Line::from(Span::styled("m create MR", Style::default().fg(DIM))),
        ],
        Some(MrSnapshot::Error(error)) => vec![
            Line::from(Span::styled(
                "Merge request error",
                Style::default().fg(FAIL),
            )),
            Line::from(Span::styled(error.clone(), Style::default().fg(DIM))),
        ],
        Some(MrSnapshot::Ready(mr)) => render_mr(mr),
    }
}

fn render_mr(mr: &MergeRequest) -> Vec<Line<'static>> {
    let display = classify(Some(mr));
    let display_style = match display.kind {
        MrDisplayKind::None => Style::default().fg(DIM),
        MrDisplayKind::Unknown | MrDisplayKind::Blocked => Style::default().fg(FAIL),
        MrDisplayKind::Draft | MrDisplayKind::Open => Style::default().fg(BUSY),
        MrDisplayKind::Ready | MrDisplayKind::Merged => Style::default().fg(OK),
    };
    let dim = Style::default().fg(DIM);
    let text = Style::default().fg(TEXT);

    let id = mr
        .iid
        .map(|iid| format!("!{iid}"))
        .unwrap_or_else(|| "!?".to_string());
    let title = mr.title.as_deref().unwrap_or("(untitled)");
    let state = match &mr.state {
        MrState::None => "none".to_string(),
        MrState::Open => "open".to_string(),
        MrState::Closed => "closed".to_string(),
        MrState::Merged => "merged".to_string(),
        MrState::Unknown(s) => s.clone(),
    };
    let draft = match mr.draft {
        Some(true) => "draft",
        Some(false) => "ready",
        None => "draft?",
    };
    let target = mr.target_branch.as_deref().unwrap_or("?");
    let merge = mr.merge_state.as_deref().unwrap_or("merge?");
    let pipeline = mr.pipeline_state.as_deref().unwrap_or("pipeline?");
    let unresolved = mr
        .unresolved_count
        .map(|n| n.to_string())
        .unwrap_or_else(|| "?".to_string());

    let mut lines = vec![
        Line::from(vec![
            Span::styled(display.glyph, display_style),
            Span::styled(" ", dim),
            Span::styled(id, text.add_modifier(Modifier::BOLD)),
            Span::styled(" ", dim),
            Span::styled(title.to_string(), text),
        ]),
        Line::from(vec![
            Span::styled("state ", dim),
            Span::styled(state, text),
            Span::styled("  draft ", dim),
            Span::styled(draft, text),
            Span::styled("  merge ", dim),
            Span::styled(merge.to_string(), text),
            Span::styled("  ci ", dim),
            Span::styled(pipeline.to_string(), text),
            Span::styled("  notes ", dim),
            Span::styled(unresolved, text),
        ]),
        Line::from(vec![
            Span::styled("branch ", dim),
            Span::styled(mr.source_branch.clone(), text),
            Span::styled(" -> ", dim),
            Span::styled(target.to_string(), text),
        ]),
    ];

    if let Some(url) = &mr.url {
        lines.push(Line::from(vec![
            Span::styled("url ", dim),
            Span::styled(url.clone(), text),
        ]));
    }

    lines
}

fn selected_agent_keymap_items(app: &App) -> Vec<(&'static str, &'static str)> {
    let mut items = match app.selected_mr_snapshot() {
        Some(MrSnapshot::Ready(mr)) => match classify(Some(mr)).kind {
            MrDisplayKind::Ready => vec![("M", "merge"), ("o", "open"), ("r", "rebase")],
            MrDisplayKind::Blocked | MrDisplayKind::Draft | MrDisplayKind::Open => {
                vec![
                    ("f", "make-ready"),
                    ("r", "rebase"),
                    ("v", "review-fix"),
                    ("o", "open"),
                ]
            }
            _ => vec![("m", "MR"), ("o", "open")],
        },
        Some(MrSnapshot::Error(_)) => vec![("m", "retry"), ("d", "delete")],
        None | Some(MrSnapshot::Missing) => vec![("m", "create MR"), ("d", "delete")],
    };
    let preview_label = match app.preview_mode {
        PreviewMode::Terminal => "MR",
        PreviewMode::MergeRequest => "session",
    };
    items.push(("tab", preview_label));
    items.push(("q", "quit"));
    items.push(("?", "hide"));
    items
}

fn draw_agent_table(frame: &mut Frame, app: &App, area: Rect) {
    let mr_kinds: Vec<MrDisplayKind> = app
        .agents
        .iter()
        .map(|agent| match app.mr_snapshot_for_agent(agent) {
            Some(MrSnapshot::Ready(mr)) => classify(Some(mr)).kind,
            Some(MrSnapshot::Error(_)) => MrDisplayKind::Unknown,
            None | Some(MrSnapshot::Missing) => MrDisplayKind::None,
        })
        .collect();

    let empty_message = if app.config.resolved_repos().is_empty() {
        "No repos configured. Add repos to ~/.config/z/config.toml"
    } else {
        "No agents yet."
    };

    let widget = AgentTableWidget::new(&app.agents)
        .mr_kinds(&mr_kinds)
        .selected(app.selected)
        .spinner_frame(app.spinner_frame)
        .empty_message(empty_message);
    frame.render_widget(&widget, area);
}

#[derive(Clone, Copy)]
struct SeparatorPosition {
    color: Color,
    selected: bool,
}

#[derive(Clone, Copy)]
enum SeparatorLabel<'a> {
    Branch { branch: &'a str },
    Drifted { slug: &'a str, branch: &'a str },
}

struct SeparatorWidget<'a> {
    label: Option<SeparatorLabel<'a>>,
    positions: &'a [SeparatorPosition],
    has_new_agent_candidate: bool,
}

impl<'a> SeparatorWidget<'a> {
    const fn new() -> Self {
        Self {
            label: None,
            positions: &[],
            has_new_agent_candidate: false,
        }
    }

    const fn label(mut self, label: SeparatorLabel<'a>) -> Self {
        self.label = Some(label);
        self
    }

    const fn positions(mut self, positions: &'a [SeparatorPosition]) -> Self {
        self.positions = positions;
        self
    }

    const fn new_agent_candidate(mut self, has_candidate: bool) -> Self {
        self.has_new_agent_candidate = has_candidate;
        self
    }

    fn label_spans(&self) -> Option<Vec<Span<'a>>> {
        let dim_style = Style::default().fg(DIM);
        let label_style = Style::default().fg(TEXT);

        let mut spans = vec![Span::styled(" ", dim_style)];
        match self.label? {
            SeparatorLabel::Branch { branch } => {
                spans.push(Span::styled(branch, label_style));
            }
            SeparatorLabel::Drifted { slug, branch } => {
                spans.push(Span::styled(slug, label_style));
                spans.push(drift_arrow());
                spans.push(Span::styled(
                    branch,
                    label_style.add_modifier(Modifier::ITALIC),
                ));
            }
        }
        spans.push(Span::styled(" ", dim_style));
        Some(spans)
    }

    fn position_spans(&self) -> Option<Vec<Span<'a>>> {
        if self.positions.is_empty() && !self.has_new_agent_candidate {
            return None;
        }

        let dim_style = Style::default().fg(DIM);
        let mut spans = vec![Span::styled(" ", dim_style)];
        for (i, position) in self.positions.iter().enumerate() {
            let glyph = if position.selected {
                "\u{25CF}"
            } else {
                "\u{2022}"
            };
            spans.push(Span::styled(glyph, Style::default().fg(position.color)));
            if i + 1 < self.positions.len() {
                spans.push(Span::styled(" ", dim_style));
            }
        }
        if self.has_new_agent_candidate {
            if !self.positions.is_empty() {
                spans.push(Span::styled(" ", dim_style));
            }
            spans.push(Span::styled("\u{25E6}", dim_style));
        }
        spans.push(Span::styled(" ", dim_style));
        Some(spans)
    }

    fn line(&self, width: usize) -> Line<'a> {
        let dash_style = Style::default().fg(DIM);
        let label_spans = self.label_spans();
        let position_spans = self.position_spans();

        match (label_spans, position_spans) {
            (Some(label), Some(pos)) => {
                let label_len: usize = label.iter().map(|s| s.width()).sum();
                let pos_len: usize = pos.iter().map(|s| s.width()).sum();
                let left_dashes = 3;
                let right_dashes = 3;
                let middle_dashes =
                    width.saturating_sub(left_dashes + pos_len + label_len + right_dashes);
                let mut spans = vec![Span::styled("\u{2500}".repeat(left_dashes), dash_style)];
                spans.extend(pos);
                spans.push(Span::styled("\u{2500}".repeat(middle_dashes), dash_style));
                spans.extend(label);
                spans.push(Span::styled("\u{2500}".repeat(right_dashes), dash_style));
                Line::from(spans)
            }
            (Some(label), None) => {
                let label_len: usize = label.iter().map(|s| s.width()).sum();
                let right_dashes = 3;
                let left_dashes = width.saturating_sub(label_len + right_dashes);
                let mut spans = vec![Span::styled("\u{2500}".repeat(left_dashes), dash_style)];
                spans.extend(label);
                spans.push(Span::styled("\u{2500}".repeat(right_dashes), dash_style));
                Line::from(spans)
            }
            (None, Some(pos)) => {
                let pos_len: usize = pos.iter().map(|s| s.width()).sum();
                let left_dashes = 3;
                let right_dashes = width.saturating_sub(left_dashes + pos_len);
                let mut spans = vec![Span::styled("\u{2500}".repeat(left_dashes), dash_style)];
                spans.extend(pos);
                spans.push(Span::styled("\u{2500}".repeat(right_dashes), dash_style));
                Line::from(spans)
            }
            (None, None) => Line::from(Span::styled("\u{2500}".repeat(width), dash_style)),
        }
    }
}

impl Widget for &SeparatorWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.line(area.width as usize)).render(area, buf);
    }
}

fn draw_separator(frame: &mut Frame, app: &App, area: Rect) {
    let label = app.selected_agent().map(|agent| {
        let drifted = agent.slug != agent.branch.replace('/', "-");
        if drifted {
            SeparatorLabel::Drifted {
                slug: agent.slug.as_str(),
                branch: agent.branch.as_str(),
            }
        } else {
            SeparatorLabel::Branch {
                branch: agent.branch.as_str(),
            }
        }
    });
    let positions: Vec<SeparatorPosition> = app
        .agents
        .iter()
        .enumerate()
        .map(|(i, agent)| SeparatorPosition {
            color: status_color(agent),
            selected: i == app.selected,
        })
        .collect();
    let mut widget = SeparatorWidget::new()
        .positions(&positions)
        .new_agent_candidate(matches!(app.mode, Mode::NewAgent(_)));
    if let Some(label) = label {
        widget = widget.label(label);
    }

    frame.render_widget(&widget, area);
}

struct ModalFrame<'a> {
    title: &'a str,
}

impl<'a> ModalFrame<'a> {
    const fn new(title: &'a str) -> Self {
        Self { title }
    }

    fn block(&self) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(DIM))
            .title(modal_title(self.title))
    }

    fn inner(&self, area: Rect) -> Rect {
        self.block().inner(area)
    }

    fn render(&self, frame: &mut Frame, area: Rect) -> Rect {
        let inner = self.inner(area);
        frame.render_widget(self, area);
        inner
    }
}

impl Widget for &ModalFrame<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.block().render(area, buf);
    }
}

fn tail_lines(s: &str, n: usize) -> &str {
    let mut count = 0;
    for (i, _) in s.rmatch_indices('\n') {
        count += 1;
        if count == n {
            return &s[i + 1..];
        }
    }
    s
}

fn draw_blank_pane_placeholder(frame: &mut Frame, area: Rect) {
    let art_width = BLANK_PANE_Z
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0) as u16;
    let art_height = BLANK_PANE_Z.len() as u16;
    let width = art_width.min(area.width);
    let height = art_height.min(area.height);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let lines: Vec<Line<'static>> = BLANK_PANE_Z
        .iter()
        .map(|line| Line::from((*line).to_string()))
        .collect();

    let preview = Paragraph::new(lines).style(Style::default().fg(DIM));
    frame.render_widget(preview, Rect::new(x, y, width, height));
}

fn draw_preview(frame: &mut Frame, app: &App, area: Rect) {
    if app.preview_mode == PreviewMode::MergeRequest {
        let preview = Paragraph::new(mr_preview_lines(app.selected_mr_snapshot()))
            .style(Style::default().fg(TEXT));
        frame.render_widget(preview, area);
        return;
    }

    let content = app.preview_content.as_deref().unwrap_or("");
    if content.trim().is_empty() {
        draw_blank_pane_placeholder(frame, area);
        return;
    }

    let tail = tail_lines(content.trim_end(), area.height as usize);
    let preview = Paragraph::new(tail).style(Style::default().fg(TEXT));

    frame.render_widget(preview, area);
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let line = if let Some(msg) = &app.status_message {
        Line::from(Span::styled(msg.as_str(), Style::default().fg(DIM)))
    } else if !app.keymap_visible {
        Line::from(Span::styled("?", Style::default().fg(DIM)))
    } else if let Mode::NewAgent(state) = &app.mode {
        crate::panel::wizard_hint(&state.focus)
    } else if app.selected_agent().is_some() {
        footer_hint(&selected_agent_keymap_items(app))
    } else {
        footer_hint(&[("n", "new"), ("?", "hide"), ("q", "quit")])
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_delete_modal(frame: &mut Frame, app: &App, area: Rect) {
    let inner = ModalFrame::new("Delete Agent").render(frame, area);

    let agent = app.selected_agent();
    let name = agent.map(|a| a.branch.as_str()).unwrap_or("?");
    let has_session = agent.is_some_and(|a| a.status.has_session());

    let layout = Layout::vertical([
        Constraint::Length(1), // top padding
        Constraint::Length(1), // line 1
        Constraint::Length(1), // line 2
        Constraint::Length(1), // line 3
        Constraint::Min(0),    // spacer
        Constraint::Length(1), // hint bar
    ]);
    let [_top_padding, line_1, line_2, line_3, _spacer, hint_bar] = inner.layout(&layout);

    let msg1 = Line::from(Span::styled(
        "  Delete worktree and branch for",
        Style::default().fg(TEXT),
    ));
    let msg2 = Line::from(vec![
        Span::styled("  ", Style::default().fg(TEXT)),
        Span::styled(name, Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
        Span::styled("?", Style::default().fg(TEXT)),
    ]);
    let msg3 = if has_session {
        Line::from(Span::styled(
            "  Default: clean up tmux session.",
            Style::default().fg(DIM),
        ))
    } else {
        Line::from(Span::styled(
            "  No active tmux session.",
            Style::default().fg(DIM),
        ))
    };
    frame.render_widget(Paragraph::new(msg1), line_1);
    frame.render_widget(Paragraph::new(msg2), line_2);
    frame.render_widget(Paragraph::new(msg3), line_3);

    let hint = if has_session {
        footer_hint(&[("y", "delete"), ("p", "keep tmux"), ("esc", "cancel")])
    } else {
        footer_hint(&[("y", "delete"), ("esc", "cancel")])
    };
    let mut spans = vec![Span::raw("  ")];
    spans.extend(hint.spans);
    frame.render_widget(Paragraph::new(Line::from(spans)), hint_bar);
}

fn draw_merge_modal(frame: &mut Frame, app: &App, area: Rect) {
    let inner = ModalFrame::new("Merge MR").render(frame, area);

    let Mode::ConfirmMerge {
        id_or_branch,
        title,
        ..
    } = &app.mode
    else {
        return;
    };
    let id = id_or_branch
        .parse::<u64>()
        .map(|iid| format!("!{iid}"))
        .unwrap_or_else(|_| id_or_branch.clone());

    let layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ]);
    let [
        _top_padding,
        prompt_row,
        mr_row,
        detail_row,
        _spacer,
        hint_bar,
    ] = inner.layout(&layout);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  Merge this merge request?",
            Style::default().fg(TEXT),
        ))),
        prompt_row,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  ", Style::default().fg(TEXT)),
            Span::styled(id, Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().fg(DIM)),
            Span::styled(title.clone(), Style::default().fg(TEXT)),
        ])),
        mr_row,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  glab will merge it upstream.",
            Style::default().fg(DIM),
        ))),
        detail_row,
    );

    let hint = footer_hint(&[("y", "merge"), ("esc", "cancel")]);
    let mut spans = vec![Span::raw("  ")];
    spans.extend(hint.spans);
    frame.render_widget(Paragraph::new(Line::from(spans)), hint_bar);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, AgentStatus};
    use crate::app::Action;
    use crate::app::{
        BranchMode, Mode, MrKey, NewAgent, NewAgentFocus, NewAgentSource, RemoteList,
    };
    use crate::config::Config;
    use crate::gitlab::{GitlabIssue, GitlabMergeRequest};
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

    fn test_app() -> App {
        let toml_str = r#"repos = ["/tmp/myapp"]"#;
        let config = Config::from_toml_str(toml_str).unwrap();
        App::new(config)
    }

    fn mock_agent(branch: &str) -> Agent {
        let slug = branch.replace('/', "-");
        Agent {
            repo_path: "/tmp/myapp".into(),
            repo_name: "myapp".into(),
            branch: branch.into(),
            base_branch: None,
            worktree_path: format!("/tmp/myapp-worktrees/{slug}").into(),
            slug: slug.clone(),
            session_name: format!("z-myapp-{slug}"),
            status: AgentStatus::Running,
            agent_name: "codex".into(),
            last_pane_hash: None,
            last_attached_count: Some(0),
            quiet_captures: 0,
            seen_activity_since_seed: false,
            was_spinner_visible: false,
            consecutive_emits: 0,
        }
    }

    fn buffer_text(buffer: &Buffer) -> String {
        let width = buffer.area().width as usize;
        buffer
            .content()
            .chunks(width)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn render_app(app: &App) -> String {
        let backend = TestBackend::new(80, 36);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| draw(frame, app)).unwrap();

        buffer_text(terminal.backend().buffer())
    }

    fn render_app_buffer(app: &App) -> Buffer {
        let backend = TestBackend::new(80, 36);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| draw(frame, app)).unwrap();

        terminal.backend().buffer().clone()
    }

    fn status_row_text(app: &App) -> String {
        let buffer = render_app_buffer(app);
        let y = buffer.area().height.saturating_sub(2);
        (0..buffer.area().width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
            .trim()
            .to_string()
    }

    fn render_widget_text<W: ratatui::widgets::Widget>(
        widget: W,
        width: u16,
        height: u16,
    ) -> String {
        let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
        widget.render(*buffer.area(), &mut buffer);
        buffer_text(&buffer)
    }

    fn find_text_pos(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
        let needle_width = needle.chars().count() as u16;
        for y in 0..buffer.area().height {
            for x in 0..buffer.area().width.saturating_sub(needle_width) {
                let candidate: String = (0..needle_width)
                    .map(|offset| buffer[(x + offset, y)].symbol())
                    .collect();
                if candidate == needle {
                    return Some((x, y));
                }
            }
        }
        None
    }

    fn new_agent_state_mut(app: &mut App) -> &mut NewAgent {
        match &mut app.mode {
            Mode::NewAgent(state) => state,
            _ => panic!("expected new-agent mode"),
        }
    }

    fn branch_source_app() -> App {
        let mut app = test_app();
        app.update(Action::StartNewAgent);
        {
            let state = new_agent_state_mut(&mut app);
            state.source = NewAgentSource::Branch;
            state.focus = NewAgentFocus::Source;
            state.branch_mode = BranchMode::New;
            state.branches = vec![
                "main".into(),
                "team/render-task-list".into(),
                "feat/configure-retry-env".into(),
                "team/system-version".into(),
                "search_strategy".into(),
                "fix/local-disk-pressure-cascade".into(),
            ];
            state.branch_name = "z-0506-138-feature-task-wizard-layout-polish".into();
            state.prompt.clear();
        }
        app
    }

    fn issue(iid: u64, title: &str) -> GitlabIssue {
        GitlabIssue {
            iid,
            title: title.to_string(),
            description: None,
            web_url: None,
        }
    }

    fn mr(iid: u64, title: &str, source_branch: &str) -> GitlabMergeRequest {
        GitlabMergeRequest {
            iid,
            title: title.to_string(),
            description: None,
            web_url: None,
            source_branch: source_branch.to_string(),
            target_branch: None,
        }
    }

    #[test]
    fn hidden_normal_keymap_only_shows_question_mark_toggle() {
        let mut app = test_app();
        app.agents = vec![mock_agent("fix-auth")];

        let status = status_row_text(&app);

        assert_eq!(status, "?");
    }

    #[test]
    fn visible_normal_keymap_shows_context_actions_and_hide_toggle() {
        let mut app = test_app();
        app.agents = vec![mock_agent("fix-auth")];
        app.keymap_visible = true;

        let status = status_row_text(&app);

        assert!(
            status.contains("m create MR"),
            "visible keymap should include selected-row MR action:\n{status}"
        );
        assert!(
            status.contains("? hide"),
            "visible keymap should always include the question-mark hide toggle:\n{status}"
        );
        assert!(
            status.contains("q quit"),
            "visible keymap should advertise the top-level quit key:\n{status}"
        );
    }

    #[test]
    fn separator_widget_renders_positions_label_and_dashes_from_props() {
        let positions = [
            SeparatorPosition {
                color: OK,
                selected: true,
            },
            SeparatorPosition {
                color: DIM,
                selected: false,
            },
        ];
        let widget = SeparatorWidget::new()
            .positions(&positions)
            .label(SeparatorLabel::Branch {
                branch: "feature-x",
            });

        let text = render_widget_text(&widget, 24, 1);

        assert_eq!(
            text,
            "\u{2500}\u{2500}\u{2500} \u{25CF} \u{2022} \u{2500}\u{2500} feature-x \u{2500}\u{2500}\u{2500}"
        );
    }

    #[test]
    fn visible_normal_keymap_names_tab_preview_destination() {
        let mut app = test_app();
        app.agents = vec![mock_agent("fix-auth")];
        app.keymap_visible = true;

        let status = status_row_text(&app);
        assert!(
            status.contains("tab MR"),
            "terminal preview should advertise tab as switching to MR preview:\n{status}"
        );

        app.preview_mode = PreviewMode::MergeRequest;
        let status = status_row_text(&app);
        assert!(
            status.contains("tab session"),
            "MR preview should advertise tab as switching back to the agent session:\n{status}"
        );
    }

    #[test]
    fn blank_terminal_preview_renders_ascii_z_placeholder() {
        let mut app = test_app();
        app.agents = vec![mock_agent("fix-auth")];

        let text = render_app(&app);

        assert!(
            text.contains(",----,"),
            "blank preview should render the ASCII Z top:\n{text}"
        );
        assert!(
            text.contains("./__;       :"),
            "blank preview should render the ASCII Z lower face:\n{text}"
        );
    }

    #[test]
    fn hidden_new_agent_keymap_only_shows_question_mark_toggle() {
        let mut app = test_app();
        app.update(Action::StartNewAgent);

        let status = status_row_text(&app);

        assert_eq!(status, "?");
    }

    #[test]
    fn visible_new_agent_keymap_shows_wizard_actions_and_hide_toggle() {
        let mut app = test_app();
        app.keymap_visible = true;
        app.update(Action::StartNewAgent);

        let status = status_row_text(&app);

        assert!(
            status.contains("tab next"),
            "visible wizard keymap should include current wizard actions:\n{status}"
        );
        assert!(
            status.contains("? hide"),
            "visible wizard keymap should include the global hide toggle:\n{status}"
        );
        assert!(
            !status.contains("q/esc"),
            "visible wizard keymap should not advertise q as cancel:\n{status}"
        );
    }

    #[test]
    fn visible_new_agent_list_keymap_shows_esc_cancel() {
        let mut app = test_app();
        app.keymap_visible = true;
        app.update(Action::StartNewAgent);
        new_agent_state_mut(&mut app).focus = NewAgentFocus::BranchList;

        let status = status_row_text(&app);

        assert!(
            status.contains("esc cancel"),
            "visible list keymap should advertise esc as cancel:\n{status}"
        );
        assert!(
            !status.contains("q/esc"),
            "visible list keymap should not advertise q as cancel:\n{status}"
        );
    }

    #[test]
    fn delete_modal_hint_only_names_esc_cancel() {
        let mut app = test_app();
        app.agents = vec![mock_agent("fix-auth")];
        app.update(Action::StartDelete);

        let text = render_app(&app);

        assert!(text.contains("esc cancel"));
        assert!(!text.contains("q/esc cancel"));
    }

    #[test]
    fn merge_modal_hint_only_names_esc_cancel() {
        let mut app = test_app();
        app.mode = Mode::ConfirmMerge {
            key: MrKey::new("/tmp/myapp".into(), "fix-auth".into()),
            id_or_branch: "1".into(),
            title: "Fix auth".into(),
        };

        let text = render_app(&app);

        assert!(text.contains("esc cancel"));
        assert!(!text.contains("q/esc cancel"));
    }

    #[test]
    fn modal_frame_renders_shared_chrome_and_reports_inner_rect() {
        let modal = ModalFrame::new("Merge MR");
        let area = Rect::new(0, 0, 20, 5);
        let mut buffer = Buffer::empty(area);

        let inner = modal.inner(area);
        ratatui::widgets::Widget::render(&modal, area, &mut buffer);

        assert_eq!(inner, Rect::new(1, 1, 18, 3));
        assert_eq!(buffer[(0, 0)].symbol(), "\u{250c}");
        assert_eq!(buffer[(19, 0)].symbol(), "\u{2510}");
        assert_eq!(buffer[(0, 4)].symbol(), "\u{2514}");
        assert_eq!(buffer[(19, 4)].symbol(), "\u{2518}");
        assert!(buffer_text(&buffer).contains(" Merge MR "));
    }

    #[test]
    fn new_agent_wizard_uses_preview_panel_without_hiding_agent_table() {
        let mut app = test_app();
        app.agents = vec![mock_agent("fix-auth")];
        app.update(Action::StartNewAgent);

        let text = render_app(&app);

        assert!(text.contains("Source"));
        assert!(
            text.contains("BRANCH"),
            "agent table header should remain visible while the wizard is open:\n{text}"
        );
        assert!(
            !text.contains("New Agent"),
            "wizard should not draw a centered modal title:\n{text}"
        );
    }

    #[test]
    fn new_agent_wizard_renders_source_tabs() {
        let mut app = test_app();
        app.update(Action::StartNewAgent);

        let text = render_app(&app);

        assert!(
            text.contains("Source   branch  mr  issue"),
            "source choice should expose all start modes as tabs:\n{text}"
        );
    }

    #[test]
    fn new_agent_wizard_orders_primary_controls() {
        let mut app = test_app();
        app.update(Action::StartNewAgent);

        let text = render_app(&app);

        // Labels right-aligned within LABEL_W with a 1-col right gutter.
        // Repo is focused → focus accent bar `│` sits at LABEL_W with one
        // padding col before the value. Other rows are unfocused → 2 blank
        // padding cols before the value, so 3 blanks separate label and value.
        let repo = text.find("Repo \u{2502} myapp").expect(&text);
        let source = text.find("Source   branch  mr  issue").expect(&text);
        let branch = text.find("Branch   new  existing").expect(&text);
        let name = text.find("Name").expect(&text);
        let prompt = text.find("Prompt").expect(&text);
        let agent = text.find("Agent   claude  codex").expect(&text);
        assert!(
            repo < source && source < branch && branch < name && name < prompt && prompt < agent,
            "wizard controls should be ordered Repo, Source, Search/options, Prompt, Agent:\n{text}"
        );
    }

    #[test]
    fn new_agent_wizard_renders_single_prompt_row_and_agent_tabs() {
        let mut app = test_app();
        app.update(Action::StartNewAgent);

        let text = render_app(&app);

        assert!(text.contains("Prompt"), "prompt row should render:\n{text}");
        assert!(
            !text.contains("default  custom"),
            "prompt should not render mode tabs:\n{text}"
        );
        assert!(
            text.contains("Agent   claude  codex"),
            "agent choice should render as tabs:\n{text}"
        );
    }

    #[test]
    fn issue_prompt_summary_shows_prompt_content() {
        let mut app = test_app();
        app.update(Action::StartNewAgent);
        new_agent_state_mut(&mut app).source = NewAgentSource::Issue;
        app.update(Action::GitlabIssuesLoaded {
            repo: "/tmp/myapp".into(),
            result: Ok(vec![GitlabIssue {
                iid: 42,
                title: "Fix task setup".to_string(),
                description: Some("Use setup context.".to_string()),
                web_url: None,
            }]),
        });

        let text = render_app(&app);

        assert!(
            !text.contains("default  custom"),
            "prompt should not render mode tabs:\n{text}"
        );
        assert!(
            text.contains("Work on GitLab issue #42"),
            "prompt body should preview the prompt content:\n{text}"
        );
    }

    #[test]
    fn branch_wizard_locks_prompt_body_to_three_rows_when_unfocused() {
        // The wizard's prompt body is fixed at PROMPT_BODY_HEIGHT (3) rows
        // regardless of focus. The label shares the body's first row (top-
        // aligned), so the agent row lands exactly 4 rows below the prompt
        // row: prompt (3) + divider (1) = 4.
        let app = branch_source_app();
        let text = render_app(&app);
        let lines: Vec<&str> = text.lines().collect();
        let prompt_row = lines
            .iter()
            .position(|line| line.contains("Prompt"))
            .expect(&text);
        let agent_row = lines
            .iter()
            .position(|line| line.contains("Agent"))
            .expect(&text);

        assert_eq!(
            agent_row.saturating_sub(prompt_row),
            4,
            "prompt body should always reserve 3 rows with a divider beneath; agent must sit 4 rows below the prompt row:\n{text}"
        );
    }

    #[test]
    fn prompt_summary_shows_prompt_content() {
        let mut app = branch_source_app();
        new_agent_state_mut(&mut app).prompt = "Refine wizard layout behavior".into();

        let text = render_app(&app);

        assert!(
            text.contains("Refine wizard layout behavior"),
            "collapsed prompt should preview the prompt content:\n{text}"
        );
        assert!(
            !text.contains("custom prompt"),
            "collapsed prompt should not mention a prompt mode:\n{text}"
        );
    }

    #[test]
    fn long_branch_name_is_truncated_in_name_row() {
        let app = branch_source_app();
        let full_name = "z-0506-138-feature-task-wizard-layout-polish";
        let text = render_app(&app);

        assert!(
            !text.contains(full_name),
            "full branch name should not overflow the row:\n{text}"
        );
        assert!(
            text.contains("..."),
            "truncated branch name should show an ellipsis:\n{text}"
        );
    }

    #[test]
    fn pristine_long_branch_name_is_truncated_when_name_focused() {
        let mut app = branch_source_app();
        {
            let state = new_agent_state_mut(&mut app);
            state.focus = NewAgentFocus::Name;
            state.name_pristine = true;
        }

        let full_name = "z-0506-138-feature-task-wizard-layout-polish";
        let text = render_app(&app);

        assert!(
            !text.contains(full_name),
            "focused pristine branch name should not overflow the row:\n{text}"
        );
        assert!(
            text.contains("..."),
            "focused pristine branch name should show an ellipsis:\n{text}"
        );
    }

    #[test]
    fn unfocused_branch_mode_value_stays_readable() {
        let app = branch_source_app();
        let buffer = render_app_buffer(&app);
        // Branch mode is rendered as lowercase tab segments (`new  existing`).
        // Selected segment uses TEXT regardless of row focus.
        let (x, y) = find_text_pos(&buffer, "new").expect("missing branch mode value");

        assert_eq!(buffer[(x, y)].fg, TEXT);
    }

    #[test]
    fn new_agent_wizard_adds_draft_dot_to_separator() {
        let mut app = test_app();
        app.agents = vec![mock_agent("fix-auth"), mock_agent("docs")];
        app.update(Action::StartNewAgent);

        let text = render_app(&app);

        assert!(
            text.contains("\u{25E6}"),
            "separator should include a draft candidate dot while wizard is open:\n{text}"
        );
    }

    #[test]
    fn new_agent_wizard_scrolls_selected_issue_into_view() {
        let mut app = test_app();
        app.update(Action::StartNewAgent);

        {
            let state = new_agent_state_mut(&mut app);
            state.focus = NewAgentFocus::SourceList;
            state.source = NewAgentSource::Issue;
            state.source_index = 7;
            state.issues =
                RemoteList::Loaded((1..=8).map(|n| issue(n, &format!("Issue {n}"))).collect());
        }

        let text = render_app(&app);

        assert!(
            text.contains("#8 Issue 8"),
            "selected issue should be visible after scrolling:\n{text}"
        );
        assert!(
            !text.contains("#1 Issue 1"),
            "first issue should scroll out of the six-row source list:\n{text}"
        );
    }

    #[test]
    fn new_agent_wizard_scrolls_selected_mr_into_view() {
        let mut app = test_app();
        app.update(Action::StartNewAgent);

        {
            let state = new_agent_state_mut(&mut app);
            state.focus = NewAgentFocus::SourceList;
            state.source = NewAgentSource::Mr;
            state.source_index = 7;
            let items = (1..=8)
                .map(|n| mr(n, &format!("MR {n}"), &format!("feature/mr-{n}")))
                .collect::<Vec<_>>();
            state.selected_mr = items.get(7).cloned();
            state.mrs = RemoteList::Loaded(items);
        }

        let text = render_app(&app);

        assert!(
            text.contains("!8 MR 8 feature/mr-8"),
            "selected MR should be visible after scrolling:\n{text}"
        );
        assert!(
            !text.contains("!1 MR 1 feature/mr-1"),
            "first MR should scroll out of the six-row MR list:\n{text}"
        );
    }

    #[test]
    fn new_agent_wizard_scrolls_mr_list_after_picker_next() {
        let mut app = test_app();
        app.update(Action::StartNewAgent);

        {
            let state = new_agent_state_mut(&mut app);
            state.focus = NewAgentFocus::SourceList;
            state.source = NewAgentSource::Mr;
            state.source_index = 5;
            let items = (1..=7)
                .map(|n| mr(n, &format!("MR {n}"), &format!("feature/mr-{n}")))
                .collect::<Vec<_>>();
            state.selected_mr = items.get(5).cloned();
            state.mrs = RemoteList::Loaded(items);
        }

        app.update(Action::PickerNext);
        let text = render_app(&app);

        assert!(
            text.contains("!7 MR 7 feature/mr-7"),
            "down from the last visible MR should reveal the next MR:\n{text}"
        );
        assert!(
            !text.contains("!1 MR 1 feature/mr-1"),
            "MR list should scroll instead of staying pinned to the first row:\n{text}"
        );
    }

    #[test]
    fn new_agent_wizard_scrolls_selected_branch_into_view() {
        let mut app = test_app();
        app.update(Action::StartNewAgent);

        {
            let state = new_agent_state_mut(&mut app);
            state.focus = NewAgentFocus::BranchList;
            state.source = NewAgentSource::Branch;
            state.branch_mode = BranchMode::New;
            state.base_index = 7;
            state.branches = (1..=8).map(|n| format!("branch-{n}")).collect();
        }

        let text = render_app(&app);

        assert!(
            text.contains("branch-8"),
            "selected branch should be visible after scrolling:\n{text}"
        );
        assert!(
            !text.contains("branch-1"),
            "first branch should scroll out of the six-row branch list:\n{text}"
        );
    }
}
