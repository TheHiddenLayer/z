use crate::agent::{Agent, AgentStatus};
use crate::app::{App, Mode, MrSnapshot, NewAgentSource, PreviewMode, RemoteList};
use crate::gitlab::{
    GitlabIssue, GitlabMergeRequest, MergeRequest, MrDisplayKind, MrState, classify,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
};

use crate::style::{
    BUSY, DIM, FAIL, OK, TEXT, drift_arrow, footer_hint, modal_title, status_color,
};

const AGENT_TABLE_HEIGHT: u16 = 6;
const NEW_AGENT_LABEL_W: u16 = 14;

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

pub fn draw(frame: &mut Frame, app: &App) {
    let inner = frame.area().inner(Margin {
        vertical: 1,
        horizontal: 3,
    });

    let chunks = Layout::vertical([
        Constraint::Min(1),                     // preview pane
        Constraint::Length(1),                  // breathing room above separator
        Constraint::Length(1),                  // horizontal separator
        Constraint::Length(1),                  // breathing room below separator
        Constraint::Length(AGENT_TABLE_HEIGHT), // agent table: header + gap + 4 rows
        Constraint::Length(1),                  // breathing room above status bar
        Constraint::Length(1),                  // status bar
    ])
    .split(inner);

    draw_preview(frame, app, chunks[0]);

    // chunks[1] is empty breathing room above separator
    draw_separator(frame, app, chunks[2]);
    // chunks[3] is empty breathing room below separator
    draw_agent_table(frame, app, chunks[4]);
    // chunks[5] is empty breathing room above status bar
    draw_status_bar(frame, app, chunks[6]);

    // Modal overlays
    match &app.mode {
        Mode::NewAgent { .. } => {
            let modal_area = centered_rect(60, 88, frame.area());
            frame.render_widget(Clear, modal_area);
            draw_new_agent_modal(frame, app, modal_area);
        }
        Mode::ConfirmDelete => {
            let modal_area = centered_rect(52, 28, frame.area());
            frame.render_widget(Clear, modal_area);
            draw_delete_modal(frame, app, modal_area);
        }
        Mode::ConfirmMerge { .. } => {
            let modal_area = centered_rect(54, 26, frame.area());
            frame.render_widget(Clear, modal_area);
            draw_merge_modal(frame, app, modal_area);
        }
        _ => {}
    }
}

const SPINNER_FRAMES: [&str; 10] = [
    "\u{280B}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283C}", "\u{2834}", "\u{2826}", "\u{2827}",
    "\u{2807}", "\u{280F}",
];

fn status_glyph(agent: &Agent, frame_idx: usize, _base: Style) -> Span<'static> {
    // The status glyph carries its own semantics — yellow spinner = working,
    // green ✓ = quiet/done, red ✗ = failed, dim − = stopped. The spinner→✓
    // transition is a color change as well as a glyph change so the moment
    // an agent finishes pops in peripheral vision.
    let style = Style::default().fg(status_color(agent));
    match &agent.status {
        AgentStatus::Error(_) => Span::styled("\u{2717}", style),
        AgentStatus::Stopped => Span::styled("\u{2212}", style),
        _ if agent.shows_spinner() => {
            let g = SPINNER_FRAMES[frame_idx % SPINNER_FRAMES.len()];
            Span::styled(g, style)
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

fn mr_status(app: &App, agent: &Agent, style: Style) -> Span<'static> {
    let kind = match app.mr_snapshot_for_agent(agent) {
        Some(MrSnapshot::Ready(mr)) => classify(Some(mr)).kind,
        Some(MrSnapshot::Error(_)) => MrDisplayKind::Unknown,
        None | Some(MrSnapshot::Missing) => MrDisplayKind::None,
    };
    Span::styled(mr_status_label(kind), style)
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

fn mr_status_hints(snapshot: Option<&MrSnapshot>) -> Line<'static> {
    match snapshot {
        Some(MrSnapshot::Ready(mr)) => match classify(Some(mr)).kind {
            MrDisplayKind::Ready => footer_hint(&[
                ("m", "MR"),
                ("M", "merge"),
                ("o", "open"),
                ("r", "rebase"),
                ("tab", "preview"),
            ]),
            MrDisplayKind::Blocked | MrDisplayKind::Draft | MrDisplayKind::Open => footer_hint(&[
                ("f", "make-ready"),
                ("r", "rebase"),
                ("v", "review-fix"),
                ("o", "open"),
                ("tab", "preview"),
            ]),
            _ => footer_hint(&[("m", "MR"), ("o", "open"), ("tab", "preview")]),
        },
        Some(MrSnapshot::Error(_)) => footer_hint(&[("m", "retry"), ("tab", "preview")]),
        None | Some(MrSnapshot::Missing) => {
            footer_hint(&[("m", "create MR"), ("tab", "preview"), ("?", "help")])
        }
    }
}

fn draw_agent_table(frame: &mut Frame, app: &App, area: Rect) {
    if app.agents.is_empty() {
        let repos = app.config.resolved_repos();
        let msg = if repos.is_empty() {
            "No repos configured. Add repos to ~/.config/z/config.toml"
        } else {
            "No agents yet."
        };
        let line = Line::from(Span::styled(msg, Style::default().fg(DIM)));
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    let visible_rows = (area.height as usize).saturating_sub(2);

    let offset = if visible_rows == 0 {
        0
    } else if app.selected >= visible_rows {
        app.selected - visible_rows + 1
    } else {
        0
    };

    let repo_w = app
        .agents
        .iter()
        .map(|a| a.repo_name.len())
        .max()
        .unwrap_or(0)
        .max(4) as u16;
    // Branch column may show "<slug> \u{2192} <branch>" when drifted; size for that.
    let branch_w = app
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
    let has_base = app
        .agents
        .iter()
        .any(|a| a.base_branch.as_deref().is_some_and(|b| !b.is_empty()));
    let base_col_w = if has_base {
        app.agents
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

    for (i, agent) in app
        .agents
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible_rows)
    {
        let is_selected = i == app.selected;

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
            Some(b) if !b.is_empty() => Line::from(Span::styled(b, text_style)),
            _ => Line::from(""),
        };

        let drifted = agent.slug != agent.branch.replace('/', "-");
        let branch_cell = if drifted {
            Line::from(vec![
                Span::styled(agent.slug.as_str(), text_style),
                drift_arrow(),
                Span::styled(
                    agent.branch.as_str(),
                    text_style.add_modifier(Modifier::ITALIC),
                ),
            ])
        } else {
            Line::from(Span::styled(agent.branch.as_str(), text_style))
        };

        rows.push(Row::new(vec![
            Cell::from(Span::styled(indicator, indicator_style)),
            Cell::from(status_glyph(agent, app.spinner_frame, text_style)),
            Cell::from(mr_status(app, agent, text_style)),
            Cell::from(branch_cell),
            Cell::from(base_cell),
            Cell::from(Span::styled(agent.repo_name.as_str(), text_style)),
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

    frame.render_widget(table, area);
}

fn draw_separator(frame: &mut Frame, app: &App, area: Rect) {
    let w = area.width as usize;
    let dash_style = Style::default().fg(DIM);

    let label_spans = if let Some(agent) = app.selected_agent() {
        let dim_style = Style::default().fg(DIM);
        let label_style = Style::default().fg(TEXT);

        let drifted = agent.slug != agent.branch.replace('/', "-");
        let mut spans = vec![Span::styled(" ", dim_style)];
        if drifted {
            spans.push(Span::styled(agent.slug.as_str(), label_style));
            spans.push(drift_arrow());
            spans.push(Span::styled(
                agent.branch.as_str(),
                label_style.add_modifier(Modifier::ITALIC),
            ));
        } else {
            spans.push(Span::styled(agent.branch.as_str(), label_style));
        }
        spans.push(Span::styled(" ", dim_style));
        Some(spans)
    } else {
        None
    };

    let total = app.agents.len();
    let position_spans: Option<Vec<Span>> = if total > 0 {
        let dim_style = Style::default().fg(DIM);
        let mut spans = vec![Span::styled(" ", dim_style)];
        for (i, agent) in app.agents.iter().enumerate() {
            let glyph = if i == app.selected {
                "\u{25CF}"
            } else {
                "\u{2022}"
            };
            let style = Style::default().fg(status_color(agent));
            spans.push(Span::styled(glyph, style));
            if i + 1 < total {
                spans.push(Span::styled(" ", dim_style));
            }
        }
        spans.push(Span::styled(" ", dim_style));
        Some(spans)
    } else {
        None
    };

    let sep = match (label_spans, position_spans) {
        (Some(label), Some(pos)) => {
            let label_len: usize = label.iter().map(|s| s.width()).sum();
            let pos_len: usize = pos.iter().map(|s| s.width()).sum();
            let left_dashes = 3;
            let right_dashes = 3;
            let middle_dashes = w.saturating_sub(left_dashes + pos_len + label_len + right_dashes);
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
            let left_dashes = w.saturating_sub(label_len + right_dashes);
            let mut spans = vec![Span::styled("\u{2500}".repeat(left_dashes), dash_style)];
            spans.extend(label);
            spans.push(Span::styled("\u{2500}".repeat(right_dashes), dash_style));
            Line::from(spans)
        }
        (None, Some(pos)) => {
            let pos_len: usize = pos.iter().map(|s| s.width()).sum();
            let left_dashes = 3;
            let right_dashes = w.saturating_sub(left_dashes + pos_len);
            let mut spans = vec![Span::styled("\u{2500}".repeat(left_dashes), dash_style)];
            spans.extend(pos);
            spans.push(Span::styled("\u{2500}".repeat(right_dashes), dash_style));
            Line::from(spans)
        }
        (None, None) => Line::from(Span::styled("\u{2500}".repeat(w), dash_style)),
    };

    frame.render_widget(Paragraph::new(sep), area);
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

fn draw_preview(frame: &mut Frame, app: &App, area: Rect) {
    if app.preview_mode == PreviewMode::MergeRequest {
        let preview = Paragraph::new(mr_preview_lines(app.selected_mr_snapshot()))
            .style(Style::default().fg(TEXT));
        frame.render_widget(preview, area);
        return;
    }

    let content = app.preview_content.as_deref().unwrap_or("");
    let tail = tail_lines(content.trim_end(), area.height as usize);
    let preview = Paragraph::new(tail).style(Style::default().fg(TEXT));

    frame.render_widget(preview, area);
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let line = if let Some(msg) = &app.status_message {
        Line::from(Span::styled(msg.as_str(), Style::default().fg(DIM)))
    } else if app.help_visible {
        footer_hint(&[
            ("↑/k", "up"),
            ("↓/j", "down"),
            ("n", "new"),
            ("a", "attach"),
            ("x", "stop"),
            ("d", "delete"),
            ("?", "hide"),
            ("q", "quit"),
        ])
    } else if app.selected_agent().is_some() {
        mr_status_hints(app.selected_mr_snapshot())
    } else {
        Line::from(Span::styled("?", Style::default().fg(DIM)))
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn source_label(source: NewAgentSource) -> &'static str {
    match source {
        NewAgentSource::Issue => "issue",
        NewAgentSource::Mr => "mr",
        NewAgentSource::Branch => "branch",
    }
}

fn remote_status_line(message: &str, label_w: u16) -> Line<'static> {
    Line::from(vec![
        Span::raw(" ".repeat(label_w as usize)),
        Span::styled(message.to_string(), Style::default().fg(DIM)),
    ])
}

fn matches_source_query(label: &str, query: &str) -> bool {
    let trimmed = query.trim();
    trimmed.is_empty()
        || label
            .to_ascii_lowercase()
            .contains(&trimmed.to_ascii_lowercase())
}

fn selectable_source_line(label: String, selected: bool, label_w: u16) -> Line<'static> {
    let style = if selected {
        Style::default().fg(TEXT)
    } else {
        Style::default().fg(DIM)
    };
    let indicator = if selected { "\u{2502} " } else { "  " };
    Line::from(vec![
        Span::raw(" ".repeat(label_w as usize)),
        Span::styled(indicator, style),
        Span::styled(label, style),
    ])
}

fn issue_label(issue: &GitlabIssue) -> String {
    format!("#{} {}", issue.iid, issue.title)
}

fn mr_label(mr: &GitlabMergeRequest) -> String {
    format!("!{} {} {}", mr.iid, mr.title, mr.source_branch)
}

fn filtered_issue_indices(issues: &[GitlabIssue], query: &str) -> Vec<usize> {
    issues
        .iter()
        .enumerate()
        .filter_map(|(index, issue)| {
            matches_source_query(&issue_label(issue), query).then_some(index)
        })
        .collect()
}

fn filtered_mr_indices(mrs: &[GitlabMergeRequest], query: &str) -> Vec<usize> {
    mrs.iter()
        .enumerate()
        .filter_map(|(index, mr)| matches_source_query(&mr_label(mr), query).then_some(index))
        .collect()
}

fn filtered_issue_lines(
    issues: &RemoteList<GitlabIssue>,
    query: &str,
    selected_index: usize,
    label_w: u16,
) -> Vec<Line<'static>> {
    match issues {
        RemoteList::Idle | RemoteList::Loading => {
            vec![remote_status_line("loading assigned issues...", label_w)]
        }
        RemoteList::Failed(message) => {
            vec![remote_status_line(&format!("error: {message}"), label_w)]
        }
        RemoteList::Loaded(items) => {
            let indices = filtered_issue_indices(items, query);
            if indices.is_empty() {
                let message = if items.is_empty() {
                    "no assigned issues"
                } else {
                    "no matching issues"
                };
                return vec![remote_status_line(message, label_w)];
            }
            indices
                .into_iter()
                .map(|index| {
                    selectable_source_line(
                        issue_label(&items[index]),
                        index == selected_index,
                        label_w,
                    )
                })
                .collect()
        }
    }
}

fn filtered_mr_lines(
    mrs: &RemoteList<GitlabMergeRequest>,
    query: &str,
    selected_index: usize,
    label_w: u16,
) -> Vec<Line<'static>> {
    match mrs {
        RemoteList::Idle | RemoteList::Loading => {
            vec![remote_status_line("loading review MRs...", label_w)]
        }
        RemoteList::Failed(message) => {
            vec![remote_status_line(&format!("error: {message}"), label_w)]
        }
        RemoteList::Loaded(items) => {
            let indices = filtered_mr_indices(items, query);
            if indices.is_empty() {
                let message = if items.is_empty() {
                    "no MRs needing review"
                } else {
                    "no matching MRs"
                };
                return vec![remote_status_line(message, label_w)];
            }
            indices
                .into_iter()
                .map(|index| {
                    selectable_source_line(
                        mr_label(&items[index]),
                        index == selected_index,
                        label_w,
                    )
                })
                .collect()
        }
    }
}

fn source_list_height(
    source: NewAgentSource,
    issues: &RemoteList<GitlabIssue>,
    mrs: &RemoteList<GitlabMergeRequest>,
    branches: &[String],
    query: &str,
) -> u16 {
    let count = match source {
        NewAgentSource::Issue => match issues {
            RemoteList::Loaded(items) => filtered_issue_indices(items, query).len(),
            _ => 1,
        },
        NewAgentSource::Mr => match mrs {
            RemoteList::Loaded(items) => filtered_mr_indices(items, query).len(),
            _ => 1,
        },
        NewAgentSource::Branch => branches.len(),
    };
    count.min(6).max(1) as u16
}

#[derive(Debug, Clone, Copy)]
struct NewAgentLayoutSizing {
    top_padding: u16,
    gap_after_source: u16,
    gap_after_agent: u16,
    gap_after_repo: u16,
    gap_after_list: u16,
    list_height: u16,
    #[cfg(test)]
    required_non_list_height: u16,
}

impl NewAgentLayoutSizing {
    #[cfg(test)]
    fn optional_spacer_height(self) -> u16 {
        self.top_padding
            + self.gap_after_source
            + self.gap_after_agent
            + self.gap_after_repo
            + self.gap_after_list
    }

    #[cfg(test)]
    fn total_height(self) -> u16 {
        self.required_non_list_height + self.list_height + self.optional_spacer_height()
    }

    fn source_area_height(self, show_gitlab_source: bool) -> u16 {
        self.list_height + u16::from(show_gitlab_source)
    }
}

fn new_agent_layout_sizing(
    inner_height: u16,
    desired_list_height: u16,
    show_gitlab_source: bool,
    show_branch_toggle: bool,
    show_name_row: bool,
) -> NewAgentLayoutSizing {
    let required_non_list_height = 8
        + u16::from(show_gitlab_source)
        + u16::from(show_branch_toggle)
        + u16::from(show_name_row);
    let available_for_list = inner_height.saturating_sub(required_non_list_height).max(1);
    let list_height = desired_list_height.clamp(1, 6).min(available_for_list);
    let mut optional_space = inner_height.saturating_sub(required_non_list_height + list_height);

    let mut take_spacer = || {
        let row = u16::from(optional_space > 0);
        optional_space = optional_space.saturating_sub(row);
        row
    };

    NewAgentLayoutSizing {
        top_padding: take_spacer(),
        gap_after_source: take_spacer(),
        gap_after_agent: take_spacer(),
        gap_after_repo: take_spacer(),
        gap_after_list: take_spacer(),
        list_height,
        #[cfg(test)]
        required_non_list_height,
    }
}

fn draw_new_agent_modal(frame: &mut Frame, app: &App, area: Rect) {
    use crate::app::{BranchMode, NewAgentFocus};

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(modal_title("New Agent"));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Mode::NewAgent {
        repo_index,
        source,
        source_query,
        source_index,
        issues,
        mrs,
        selected_issue: _,
        selected_mr: _,
        branch_mode,
        prompt,
        prompt_mode: _,
        focus,
        base_index,
        branches,
        existing_branches,
        branch_name,
        name_pristine,
        agent_name,
    } = &app.mode
    else {
        return;
    };

    let repos = app.config.resolved_repos();
    let repo_name = repos
        .get(*repo_index)
        .and_then(|r| r.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("?");

    let active_list: &[String] = match branch_mode {
        BranchMode::New => branches,
        BranchMode::Existing => existing_branches,
    };
    let label_w = NEW_AGENT_LABEL_W;
    let desired_list_height = source_list_height(*source, issues, mrs, active_list, source_query);
    let show_gitlab_source = matches!(source, NewAgentSource::Issue | NewAgentSource::Mr);
    let show_branch_controls = matches!(source, NewAgentSource::Branch | NewAgentSource::Issue);
    let show_branch_toggle = matches!(source, NewAgentSource::Branch);
    let show_name = show_branch_controls
        && matches!(branch_mode, BranchMode::New)
        && !matches!(source, NewAgentSource::Issue);
    let show_issue_name = matches!(source, NewAgentSource::Issue);
    let show_name_row = show_name || show_issue_name;
    let name_rows = u16::from(show_name_row);
    let sizing = new_agent_layout_sizing(
        inner.height,
        desired_list_height,
        show_gitlab_source,
        show_branch_toggle,
        show_name_row,
    );

    let chunks = Layout::vertical([
        Constraint::Length(sizing.top_padding),
        Constraint::Length(1), // Start from row
        Constraint::Length(sizing.gap_after_source),
        Constraint::Length(1), // Agent row
        Constraint::Length(sizing.gap_after_agent),
        Constraint::Length(1), // Repo row
        Constraint::Length(sizing.gap_after_repo),
        Constraint::Length(if show_branch_toggle { 1 } else { 0 }),
        Constraint::Length(sizing.source_area_height(show_gitlab_source)),
        Constraint::Length(sizing.gap_after_list),
        Constraint::Length(name_rows), // Name row
        Constraint::Length(1),         // Prompt label
        Constraint::Min(3),            // Prompt area
        Constraint::Length(1),         // hint bar
    ])
    .split(inner);

    let label_style = |focused: bool| {
        if focused {
            Style::default().fg(TEXT)
        } else {
            Style::default().fg(DIM)
        }
    };
    let val_style = |focused: bool| {
        if focused {
            Style::default().fg(TEXT)
        } else {
            Style::default().fg(DIM)
        }
    };

    // Picker row: "│ Label    value" when focused, "  Label    value" when not.
    // Selection is encoded by the left bar plus whole-row brightness contrast —
    // focused rows TEXT, unfocused rows DIM — matching the agent table's
    // convention. Without it the focus signal is too subtle in a vertical stack.
    let picker_row = |label: &str, value: &str, focused: bool| -> Line<'static> {
        let indicator = if focused { "\u{2502} " } else { "  " };
        let indicator_style = if focused {
            Style::default().fg(TEXT)
        } else {
            Style::default()
        };
        let row_style = if focused {
            Style::default().fg(TEXT)
        } else {
            Style::default().fg(DIM)
        };
        let label_style = row_style;
        let value_style = row_style;
        let label_field_w = label_w as usize;
        // Label occupies the label column; value starts at column label_w + 2.
        let label_padding = label_field_w.saturating_sub(label.len() + 2);
        Line::from(vec![
            Span::styled(indicator.to_string(), indicator_style),
            Span::styled(label.to_string(), label_style),
            Span::raw(" ".repeat(label_padding)),
            Span::styled(value.to_string(), value_style),
        ])
    };

    // --- Source row ---
    let is_source = matches!(focus, NewAgentFocus::Source);
    let source_line = picker_row("Start from", source_label(*source), is_source);
    frame.render_widget(Paragraph::new(source_line), chunks[1]);

    // --- Agent row ---
    let is_agent = matches!(focus, NewAgentFocus::Agent);
    let agent_line = picker_row("Agent", agent_name.as_str(), is_agent);
    frame.render_widget(Paragraph::new(agent_line), chunks[3]);

    // --- Repo row ---
    let is_repo = matches!(focus, NewAgentFocus::Repo);
    let repo_line = picker_row("Repo", repo_name, is_repo);
    frame.render_widget(Paragraph::new(repo_line), chunks[5]);

    // --- Branch toggle row ---
    if show_branch_toggle {
        let is_toggle = matches!(focus, NewAgentFocus::BranchToggle);
        let mode_label = match branch_mode {
            BranchMode::New => "New",
            BranchMode::Existing => "Existing",
        };
        let toggle_line = picker_row("Branch", mode_label, is_toggle);
        frame.render_widget(Paragraph::new(toggle_line), chunks[7]);
    }

    // --- Source or branch list ---
    let list_slot = chunks[8];
    if show_gitlab_source {
        let source_chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(sizing.list_height),
        ])
        .split(list_slot);

        let is_search = matches!(focus, NewAgentFocus::Search);
        let search_value = if source_query.is_empty() {
            match source {
                NewAgentSource::Issue => "filter issues...",
                NewAgentSource::Mr => "filter MRs...",
                NewAgentSource::Branch => "",
            }
        } else {
            source_query.as_str()
        };
        let search_line = picker_row("Search", search_value, is_search);
        frame.render_widget(Paragraph::new(search_line), source_chunks[0]);

        let list_area = source_chunks[1];
        let all_lines = match source {
            NewAgentSource::Issue => {
                filtered_issue_lines(issues, source_query, *source_index, label_w)
            }
            NewAgentSource::Mr => filtered_mr_lines(mrs, source_query, *source_index, label_w),
            NewAgentSource::Branch => Vec::new(),
        };
        let visible = list_area.height as usize;
        let selected_pos = match source {
            NewAgentSource::Issue => match issues {
                RemoteList::Loaded(items) => filtered_issue_indices(items, source_query)
                    .into_iter()
                    .position(|index| index == *source_index),
                _ => None,
            },
            NewAgentSource::Mr => match mrs {
                RemoteList::Loaded(items) => filtered_mr_indices(items, source_query)
                    .into_iter()
                    .position(|index| index == *source_index),
                _ => None,
            },
            NewAgentSource::Branch => None,
        };
        let scroll = selected_pos
            .filter(|_| visible > 0)
            .map(|pos| if pos >= visible { pos - visible + 1 } else { 0 })
            .unwrap_or(0);
        let lines: Vec<Line> = all_lines.into_iter().skip(scroll).take(visible).collect();
        frame.render_widget(Paragraph::new(lines), list_area);
    } else {
        let list_area = list_slot;
        if active_list.is_empty() {
            let empty_msg = match branch_mode {
                BranchMode::New => "loading...",
                BranchMode::Existing => "no existing branches",
            };
            frame.render_widget(
                Paragraph::new(remote_status_line(empty_msg, label_w)),
                list_area,
            );
        } else {
            let visible = list_area.height as usize;
            let scroll = if visible > 0 && *base_index >= visible {
                base_index - visible + 1
            } else {
                0
            };
            let lines: Vec<Line> = active_list
                .iter()
                .enumerate()
                .skip(scroll)
                .take(visible)
                .map(|(i, b)| selectable_source_line(b.clone(), i == *base_index, label_w))
                .collect();
            frame.render_widget(Paragraph::new(lines), list_area);
        }
    }

    // --- Name row ---
    if show_name {
        let is_name = matches!(focus, NewAgentFocus::Name);
        let name_display = if is_name && *name_pristine {
            // Pristine auto-suggested name: dim + italic so it reads as a
            // placeholder that will be replaced the moment the user types.
            Span::styled(
                branch_name.as_str(),
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            )
        } else {
            let cursor = if is_name { "_" } else { "" };
            Span::styled(format!("{branch_name}{cursor}"), val_style(is_name))
        };
        let name_line = Line::from(vec![
            Span::styled("  Name", label_style(is_name)),
            Span::raw(" ".repeat((label_w as usize).saturating_sub(6))),
            name_display,
        ]);
        frame.render_widget(Paragraph::new(name_line), chunks[10]);
    } else if show_issue_name {
        let name_line = Line::from(vec![
            Span::styled("  Name", Style::default().fg(DIM)),
            Span::raw(" ".repeat((label_w as usize).saturating_sub(6))),
            Span::styled(branch_name.as_str(), Style::default().fg(DIM)),
        ]);
        frame.render_widget(Paragraph::new(name_line), chunks[10]);
    }

    // --- Prompt label ---
    let is_prompt = matches!(focus, NewAgentFocus::Prompt);
    let prompt_label = Line::from(Span::styled("  Prompt", label_style(is_prompt)));
    frame.render_widget(Paragraph::new(prompt_label), chunks[11]);

    // --- Prompt area ---
    let prompt_area = chunks[12];
    if prompt.is_empty() {
        let placeholder = if is_prompt {
            Line::from(vec![
                Span::raw(" ".repeat(label_w as usize)),
                Span::styled("_", Style::default().fg(TEXT)),
            ])
        } else {
            Line::from(vec![
                Span::raw(" ".repeat(label_w as usize)),
                Span::styled("describe the task...", Style::default().fg(DIM)),
            ])
        };
        frame.render_widget(Paragraph::new(placeholder), prompt_area);
    } else {
        let cursor = if is_prompt { "_" } else { "" };
        let text = format!("{}{}{}", " ".repeat(label_w as usize), prompt, cursor);
        let width = prompt_area.width.max(1) as usize;
        let line_count: u16 = text
            .split('\n')
            .map(|l| {
                if l.is_empty() {
                    1
                } else {
                    ((l.len() as u16).saturating_add(width as u16 - 1)) / width as u16
                }
            })
            .sum();
        let scroll = line_count.saturating_sub(prompt_area.height);
        let paragraph = Paragraph::new(text)
            .style(Style::default().fg(TEXT))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0));
        frame.render_widget(paragraph, prompt_area);
    }

    // --- Hint bar ---
    let hint_line = match focus {
        NewAgentFocus::Source
        | NewAgentFocus::Agent
        | NewAgentFocus::Repo
        | NewAgentFocus::BranchToggle => {
            footer_hint(&[("←/→", "cycle"), ("tab", "next"), ("q/esc", "cancel")])
        }
        NewAgentFocus::Search => {
            footer_hint(&[("type", "filter"), ("tab", "list"), ("esc", "cancel")])
        }
        NewAgentFocus::SourceList | NewAgentFocus::BranchList => footer_hint(&[
            ("↑/k", "up"),
            ("↓/j", "down"),
            ("enter", "start"),
            ("tab", "next"),
        ]),
        NewAgentFocus::Name => footer_hint(&[("tab", "next"), ("esc", "cancel")]),
        NewAgentFocus::Prompt => footer_hint(&[
            ("enter", "start"),
            ("alt+enter", "newline"),
            ("ctrl+r", "reset"),
            ("esc", "cancel"),
        ]),
    };
    // Indent the hint line under the form's value column for visual continuity.
    let mut spans = vec![Span::raw(" ".repeat(label_w as usize))];
    spans.extend(hint_line.spans);
    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[13]);
}

fn draw_delete_modal(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(modal_title("Delete Agent"));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let agent = app.selected_agent();
    let name = agent.map(|a| a.branch.as_str()).unwrap_or("?");
    let has_session = agent.is_some_and(|a| a.status.has_session());

    let chunks = Layout::vertical([
        Constraint::Length(1), // top padding
        Constraint::Length(1), // line 1
        Constraint::Length(1), // line 2
        Constraint::Length(1), // line 3
        Constraint::Min(0),    // spacer
        Constraint::Length(1), // hint bar
    ])
    .split(inner);

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
    frame.render_widget(Paragraph::new(msg1), chunks[1]);
    frame.render_widget(Paragraph::new(msg2), chunks[2]);
    frame.render_widget(Paragraph::new(msg3), chunks[3]);

    let hint = if has_session {
        footer_hint(&[
            ("y", "delete + tmux"),
            ("p", "preserve tmux"),
            ("q/esc", "cancel"),
        ])
    } else {
        footer_hint(&[("y", "delete"), ("q/esc", "cancel")])
    };
    let mut spans = vec![Span::raw("  ")];
    spans.extend(hint.spans);
    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[5]);
}

fn draw_merge_modal(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(modal_title("Merge MR"));
    let inner = block.inner(area);
    frame.render_widget(block, area);

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

    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  Merge this merge request?",
            Style::default().fg(TEXT),
        ))),
        chunks[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  ", Style::default().fg(TEXT)),
            Span::styled(id, Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().fg(DIM)),
            Span::styled(title.clone(), Style::default().fg(TEXT)),
        ])),
        chunks[2],
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  glab will merge it upstream.",
            Style::default().fg(DIM),
        ))),
        chunks[3],
    );

    let hint = footer_hint(&[("y", "merge"), ("q/esc", "cancel")]);
    let mut spans = vec![Span::raw("  ")];
    spans.extend(hint.spans);
    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[5]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{NewAgentSource, RemoteList};
    use crate::gitlab::{GitlabIssue, GitlabMergeRequest};

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
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
    fn source_label_returns_lowercase_gitlab_source_names() {
        assert_eq!(source_label(NewAgentSource::Issue), "issue");
        assert_eq!(source_label(NewAgentSource::Mr), "mr");
        assert_eq!(source_label(NewAgentSource::Branch), "branch");
    }

    #[test]
    fn filtered_issue_lines_render_number_title_and_selection() {
        let issues = RemoteList::Loaded(vec![
            issue(123, "Fix agent startup"),
            issue(456, "Document setup"),
        ]);

        let lines = filtered_issue_lines(&issues, "agent", 0, 4);

        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), "    \u{2502} #123 Fix agent startup");
    }

    #[test]
    fn filtered_issue_lines_distinguish_empty_from_no_matches() {
        let empty = filtered_issue_lines(&RemoteList::Loaded(vec![]), "", 0, 2);
        assert_eq!(line_text(&empty[0]), "  no assigned issues");

        let issues = RemoteList::Loaded(vec![issue(123, "Fix agent startup")]);
        let no_match = filtered_issue_lines(&issues, "billing", 0, 2);
        assert_eq!(line_text(&no_match[0]), "  no matching issues");
    }

    #[test]
    fn filtered_mr_lines_include_source_branch() {
        let mrs = RemoteList::Loaded(vec![
            mr(7, "Review renderer", "feature/render"),
            mr(8, "Update docs", "docs/readme"),
        ]);

        let lines = filtered_mr_lines(&mrs, "render", 0, 2);

        assert_eq!(lines.len(), 1);
        assert_eq!(
            line_text(&lines[0]),
            "  \u{2502} !7 Review renderer feature/render"
        );
    }

    #[test]
    fn filtered_mr_lines_distinguish_empty_from_no_matches() {
        let empty = filtered_mr_lines(&RemoteList::Loaded(vec![]), "", 0, 2);
        assert_eq!(line_text(&empty[0]), "  no MRs needing review");

        let mrs = RemoteList::Loaded(vec![mr(7, "Review renderer", "feature/render")]);
        let no_match = filtered_mr_lines(&mrs, "billing", 0, 2);
        assert_eq!(line_text(&no_match[0]), "  no matching MRs");
    }

    #[test]
    fn remote_status_line_is_indented() {
        let line = remote_status_line("loading assigned issues...", 3);

        assert_eq!(line_text(&line), "   loading assigned issues...");
    }

    #[test]
    fn layout_sizing_caps_list_to_one_when_inner_height_is_tight() {
        let sizing = new_agent_layout_sizing(11, 6, true, false, true);

        assert_eq!(sizing.list_height, 1);
        assert_eq!(sizing.total_height(), 11);
        assert_eq!(sizing.optional_spacer_height(), 0);
    }

    #[test]
    fn layout_sizing_preserves_required_rows_at_minimum_supported_height() {
        let sizing = new_agent_layout_sizing(12, 6, true, true, true);

        assert_eq!(sizing.list_height, 1);
        assert_eq!(sizing.total_height(), 12);
        assert_eq!(sizing.optional_spacer_height(), 0);
    }

    #[test]
    fn layout_sizing_allows_six_list_rows_when_height_is_available() {
        let sizing = new_agent_layout_sizing(21, 6, true, false, true);

        assert_eq!(sizing.list_height, 6);
        assert_eq!(sizing.total_height(), 21);
        assert_eq!(sizing.optional_spacer_height(), 5);
    }
}
