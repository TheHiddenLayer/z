use crate::agent::{Agent, AgentStatus};
use crate::app::{App, Mode, MrSnapshot, PreviewMode};
use crate::gitlab::{MergeRequest, MrDisplayKind, MrState, classify};
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
            let modal_area = centered_rect(60, 70, frame.area());
            frame.render_widget(Clear, modal_area);
            draw_new_agent_modal(frame, app, modal_area);
        }
        Mode::ConfirmDelete => {
            let modal_area = centered_rect(52, 28, frame.area());
            frame.render_widget(Clear, modal_area);
            draw_delete_modal(frame, app, modal_area);
        }
        Mode::ConfirmMerge => {
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

fn mr_glyph(app: &App, agent: &Agent) -> Span<'static> {
    let display = classify(app.mr_for_agent(agent));
    let color = match display.kind {
        MrDisplayKind::None => DIM,
        MrDisplayKind::Unknown => FAIL,
        MrDisplayKind::Draft | MrDisplayKind::Open => BUSY,
        MrDisplayKind::Ready | MrDisplayKind::Merged => OK,
        MrDisplayKind::Blocked => FAIL,
    };
    Span::styled(display.glyph, Style::default().fg(color))
}

fn mr_preview_lines(snapshot: Option<&MrSnapshot>) -> Vec<Line<'static>> {
    match snapshot {
        None | Some(MrSnapshot::Missing) => vec![
            Line::from(Span::styled("No merge request.", Style::default().fg(DIM))),
            Line::from(Span::styled("m create", Style::default().fg(DIM))),
        ],
        Some(MrSnapshot::Refreshing) => vec![Line::from(Span::styled(
            "Refreshing merge request...",
            Style::default().fg(BUSY),
        ))],
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
    let mr_w: u16 = 1;

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
            Cell::from(mr_glyph(app, agent)),
            Cell::from(branch_cell),
            Cell::from(base_cell),
            Cell::from(Span::styled(agent.repo_name.as_str(), text_style)),
        ]));
    }

    let hdr_style = Style::default().fg(DIM);
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
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
        footer_hint(&[
            ("tab", "preview"),
            ("m", "mr"),
            ("o", "open"),
            ("r", "rebase"),
            ("f", "ready"),
            ("v", "review"),
            ("M", "merge"),
            ("?", "help"),
        ])
    } else {
        Line::from(Span::styled("?", Style::default().fg(DIM)))
    };
    frame.render_widget(Paragraph::new(line), area);
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
        branch_mode,
        prompt,
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
    let list_height = active_list.len().min(6).max(1) as u16;

    let show_name = matches!(branch_mode, BranchMode::New);
    let name_rows = if show_name { 2 } else { 0 }; // row + gap

    let chunks = Layout::vertical([
        Constraint::Length(1),           // top padding
        Constraint::Length(1),           // Agent row
        Constraint::Length(1),           // gap
        Constraint::Length(1),           // Repo row
        Constraint::Length(1),           // gap
        Constraint::Length(1),           // Branch toggle row
        Constraint::Length(list_height), // Branch list
        Constraint::Length(1),           // gap
        Constraint::Length(name_rows),   // Name row (0 if Existing)
        Constraint::Length(1),           // Prompt label
        Constraint::Min(3),              // Prompt area
        Constraint::Length(1),           // hint bar
    ])
    .split(inner);

    let label_w = 14u16;
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

    // --- Agent row ---
    let is_agent = matches!(focus, NewAgentFocus::Agent);
    let agent_line = picker_row("Agent", agent_name.as_str(), is_agent);
    frame.render_widget(Paragraph::new(agent_line), chunks[1]);

    // --- Repo row ---
    let is_repo = matches!(focus, NewAgentFocus::Repo);
    let repo_line = picker_row("Repo", repo_name, is_repo);
    frame.render_widget(Paragraph::new(repo_line), chunks[3]);

    // --- Branch toggle row ---
    let is_toggle = matches!(focus, NewAgentFocus::BranchToggle);
    let mode_label = match branch_mode {
        BranchMode::New => "New",
        BranchMode::Existing => "Existing",
    };
    let toggle_line = picker_row("Branch", mode_label, is_toggle);
    frame.render_widget(Paragraph::new(toggle_line), chunks[5]);

    // --- Branch list ---
    let list_area = chunks[6];
    if active_list.is_empty() {
        let empty_msg = match branch_mode {
            BranchMode::New => "loading...",
            BranchMode::Existing => "no existing branches",
        };
        let line = Line::from(vec![
            Span::raw(" ".repeat(label_w as usize)),
            Span::styled(empty_msg, Style::default().fg(DIM)),
        ]);
        frame.render_widget(Paragraph::new(line), list_area);
    } else {
        let visible = list_area.height as usize;
        let scroll = if *base_index >= visible {
            base_index - visible + 1
        } else {
            0
        };
        let lines: Vec<Line> = active_list
            .iter()
            .enumerate()
            .skip(scroll)
            .take(visible)
            .map(|(i, b)| {
                let selected = i == *base_index;
                let indicator = if selected { "\u{2502} " } else { "  " };
                let style = if selected {
                    Style::default().fg(TEXT)
                } else {
                    Style::default().fg(DIM)
                };
                Line::from(vec![
                    Span::raw(" ".repeat(label_w as usize)),
                    Span::styled(indicator, style),
                    Span::styled(b.as_str(), style),
                ])
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), list_area);
    }

    // --- Name row (only in New mode) ---
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
        frame.render_widget(Paragraph::new(name_line), chunks[8]);
    }

    // --- Prompt label ---
    let is_prompt = matches!(focus, NewAgentFocus::Prompt);
    let prompt_label = Line::from(Span::styled("  Prompt", label_style(is_prompt)));
    frame.render_widget(Paragraph::new(prompt_label), chunks[9]);

    // --- Prompt area ---
    let prompt_area = chunks[10];
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
        NewAgentFocus::Agent | NewAgentFocus::Repo | NewAgentFocus::BranchToggle => {
            footer_hint(&[("←/→", "cycle"), ("tab", "next"), ("q/esc", "cancel")])
        }
        NewAgentFocus::BranchList => footer_hint(&[
            ("↑/k", "up"),
            ("↓/j", "down"),
            ("tab", "next"),
            ("q/esc", "cancel"),
        ]),
        NewAgentFocus::Name => {
            footer_hint(&[("tab", "next"), ("esc", "cancel")]) // q types literally here
        }
        NewAgentFocus::Prompt => {
            footer_hint(&[
                ("enter", "start"),
                ("alt+enter", "newline"),
                ("tab", "options"),
                ("esc", "cancel"), // q types literally here
            ])
        }
    };
    // Indent the hint line under the form's value column for visual continuity.
    let mut spans = vec![Span::raw(" ".repeat(label_w as usize))];
    spans.extend(hint_line.spans);
    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[11]);
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

    let mr = app.selected_mr();
    let title = mr.and_then(|m| m.title.as_deref()).unwrap_or("?");
    let id = mr
        .and_then(|m| m.iid)
        .map(|iid| format!("!{iid}"))
        .unwrap_or_else(|| "!?".to_string());

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
            Span::styled(title.to_string(), Style::default().fg(TEXT)),
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
    use crate::app::MrSnapshot;
    use crate::gitlab::{MergeRequest, MrState};

    #[test]
    fn mr_preview_missing_prompts_creation() {
        let lines = mr_preview_lines(Some(&MrSnapshot::Missing));

        assert_eq!(lines[0].spans[0].content, "No merge request.");
    }

    #[test]
    fn mr_preview_ready_renders_compact_details() {
        let mr = test_mr();
        let lines = mr_preview_lines(Some(&MrSnapshot::Ready(mr)));

        assert!(
            lines
                .iter()
                .any(|line| { line.spans.iter().any(|span| span.content.contains("!42")) })
        );
        assert!(lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content.contains("feature/auth"))
        }));
    }

    fn test_mr() -> MergeRequest {
        MergeRequest {
            source_branch: "feature/auth".into(),
            target_branch: Some("main".into()),
            iid: Some(42),
            title: Some("Fix auth".into()),
            url: Some("https://gitlab.example.com/g/r/-/merge_requests/42".into()),
            state: MrState::Open,
            draft: Some(false),
            merge_state: Some("mergeable".into()),
            pipeline_state: Some("success".into()),
            unresolved_count: Some(0),
        }
    }
}
