use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
};
use crate::app::{App, Mode};
use crate::agent::{Agent, AgentStatus};

use crate::style::{ACCENT, BUSY, DIM, TEXT, status_color};

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
        Constraint::Min(1),                       // preview pane
        Constraint::Length(1),                    // breathing room above separator
        Constraint::Length(1),                    // horizontal separator
        Constraint::Length(1),                    // breathing room below separator
        Constraint::Length(AGENT_TABLE_HEIGHT),   // agent table: header + gap + 4 rows
        Constraint::Length(1),                    // breathing room above status bar
        Constraint::Length(1),                    // status bar
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
        _ => {}
    }
}

const SPINNER_FRAMES: [&str; 10] = [
    "\u{280B}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283C}",
    "\u{2834}", "\u{2826}", "\u{2827}", "\u{2807}", "\u{280F}",
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

fn draw_agent_table(frame: &mut Frame, app: &App, area: Rect) {
    if app.agents.is_empty() {
        let repos = app.config.resolved_repos();
        let line = if repos.is_empty() {
            Line::from(Span::styled(
                "No repos configured. Add repos to ~/.config/z/config.toml",
                Style::default().fg(DIM),
            ))
        } else {
            Line::from(vec![
                Span::styled("No agents running. Press ", Style::default().fg(DIM)),
                Span::styled("n", Style::default().fg(ACCENT)),
                Span::styled(" to create one.", Style::default().fg(DIM)),
            ])
        };
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

    let repo_w = app.agents.iter().map(|a| a.repo_name.len()).max().unwrap_or(0).max(4) as u16;
    // Branch column may show "<slug> \u{2192} <branch>" when drifted; size for that.
    let branch_w = app.agents.iter().map(|a| {
        if a.slug != a.branch.replace('/', "-") {
            a.slug.len() + 3 + a.branch.len()
        } else {
            a.branch.len()
        }
    }).max().unwrap_or(0).max(6) as u16;
    let has_base = app.agents.iter().any(|a| a.base_branch.as_deref().is_some_and(|b| !b.is_empty()));
    let base_col_w = if has_base {
        app.agents.iter()
            .map(|a| a.base_branch.as_deref().unwrap_or("").len())
            .max()
            .unwrap_or(0)
            .max(4) as u16
    } else {
        0
    };
    let status_w: u16 = 1;

    let mut rows: Vec<Row> = Vec::new();

    for (i, agent) in app.agents.iter().enumerate().skip(offset).take(visible_rows) {
        let is_selected = i == app.selected;

        let indicator = if is_selected { "\u{2502}" } else { " " };
        let indicator_style = if is_selected {
            Style::default().fg(ACCENT)
        } else {
            Style::default()
        };

        let text_style = if is_selected {
            Style::default().fg(ACCENT)
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
                Span::styled(" \u{2192} ", Style::default().fg(BUSY)),
                Span::styled(agent.branch.as_str(), text_style.add_modifier(Modifier::ITALIC)),
            ])
        } else {
            Line::from(Span::styled(agent.branch.as_str(), text_style))
        };

        rows.push(Row::new(vec![
            Cell::from(Span::styled(indicator, indicator_style)),
            Cell::from(status_glyph(agent, app.spinner_frame, text_style)),
            Cell::from(branch_cell),
            Cell::from(base_cell),
            Cell::from(Span::styled(agent.repo_name.as_str(), text_style)),
        ]));
    }

    let hdr_style = Style::default().fg(DIM);
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from(""),
        Cell::from(Span::styled("BRANCH", hdr_style)),
        Cell::from(Span::styled("BASE", hdr_style)),
        Cell::from(Span::styled("REPO", hdr_style)),
    ]).bottom_margin(1);

    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Length(status_w + 1),
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
        let accent_style = Style::default().fg(ACCENT);

        let drifted = agent.slug != agent.branch.replace('/', "-");
        let mut spans = vec![Span::styled(" ", dim_style)];
        if drifted {
            spans.push(Span::styled(agent.slug.as_str(), accent_style));
            spans.push(Span::styled(" \u{2192} ", Style::default().fg(BUSY)));
            spans.push(Span::styled(
                agent.branch.as_str(),
                accent_style.add_modifier(Modifier::ITALIC),
            ));
        } else {
            spans.push(Span::styled(agent.branch.as_str(), accent_style));
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
            let glyph = if i == app.selected { "\u{25CF}" } else { "\u{00B7}" };
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
            let middle_dashes = 3;
            let right_dashes = w
                .saturating_sub(left_dashes + pos_len + middle_dashes + label_len);
            let mut spans = vec![Span::styled("\u{2500}".repeat(left_dashes), dash_style)];
            spans.extend(pos);
            spans.push(Span::styled("\u{2500}".repeat(middle_dashes), dash_style));
            spans.extend(label);
            spans.push(Span::styled("\u{2500}".repeat(right_dashes), dash_style));
            Line::from(spans)
        }
        (Some(label), None) => {
            let label_len: usize = label.iter().map(|s| s.width()).sum();
            let left_dashes = 3;
            let right_dashes = w.saturating_sub(left_dashes + label_len);
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
    let content = app.preview_content.as_deref().unwrap_or("");
    let tail = tail_lines(content.trim_end(), area.height as usize);

    let preview = Paragraph::new(tail)
        .style(Style::default().fg(TEXT));

    frame.render_widget(preview, area);
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let line = if let Some(msg) = &app.status_message {
        Line::from(Span::styled(msg.as_str(), Style::default().fg(DIM)))
    } else {
        let key_style = Style::default().fg(TEXT).add_modifier(Modifier::BOLD);
        let label_style = Style::default().fg(DIM);
        Line::from(vec![
            Span::styled("n", key_style),
            Span::styled(" new", label_style),
            Span::styled(" \u{00b7} ", label_style),
            Span::styled("a", key_style),
            Span::styled(" attach", label_style),
            Span::styled(" \u{00b7} ", label_style),
            Span::styled("x", key_style),
            Span::styled(" stop", label_style),
            Span::styled(" \u{00b7} ", label_style),
            Span::styled("d", key_style),
            Span::styled(" delete", label_style),
            Span::styled(" \u{00b7} ", label_style),
            Span::styled("q", key_style),
            Span::styled(" quit", label_style),
        ])
    };

    let bar = Paragraph::new(line);
    frame.render_widget(bar, area);
}

fn draw_new_agent_modal(frame: &mut Frame, app: &App, area: Rect) {
    use crate::app::{NewAgentFocus, BranchMode};

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(" New Agent ", Style::default().fg(ACCENT)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Mode::NewAgent {
        repo_index, branch_mode, prompt, focus,
        base_index, branches, existing_branches,
        branch_name, name_pristine, agent_name,
    } = &app.mode else { return };

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
        Constraint::Length(1),              // top padding
        Constraint::Length(1),              // Agent row
        Constraint::Length(1),              // gap
        Constraint::Length(1),              // Repo row
        Constraint::Length(1),              // gap
        Constraint::Length(1),              // Branch toggle row
        Constraint::Length(list_height),    // Branch list
        Constraint::Length(1),              // gap
        Constraint::Length(name_rows),      // Name row (0 if Existing)
        Constraint::Length(1),              // Prompt label
        Constraint::Min(3),                // Prompt area
        Constraint::Length(1),              // hint bar
    ])
    .split(inner);

    let label_w = 14u16;
    let label_style = |focused: bool| {
        if focused { Style::default().fg(ACCENT) } else { Style::default().fg(DIM) }
    };
    let val_style = |focused: bool| {
        if focused { Style::default().fg(ACCENT) } else { Style::default().fg(TEXT) }
    };

    // --- Agent row ---
    let is_agent = matches!(focus, NewAgentFocus::Agent);
    let kind_label = agent_name.as_str();
    let agent_line = Line::from(vec![
        Span::styled("  Agent", label_style(is_agent)),
        Span::raw(" ".repeat((label_w as usize).saturating_sub(7))),
        Span::styled("\u{2039} ", Style::default().fg(if is_agent { ACCENT } else { DIM })),
        Span::styled(kind_label, val_style(is_agent)),
        Span::styled(" \u{203a}", Style::default().fg(if is_agent { ACCENT } else { DIM })),
    ]);
    frame.render_widget(Paragraph::new(agent_line), chunks[1]);

    // --- Repo row ---
    let is_repo = matches!(focus, NewAgentFocus::Repo);
    let repo_arrows = if repos.len() > 1 { ("\u{2039} ", " \u{203a}") } else { ("", "") };
    let repo_line = Line::from(vec![
        Span::styled("  Repo", label_style(is_repo)),
        Span::raw(" ".repeat((label_w as usize).saturating_sub(6))),
        Span::styled(repo_arrows.0, Style::default().fg(if is_repo { ACCENT } else { DIM })),
        Span::styled(repo_name, val_style(is_repo)),
        Span::styled(repo_arrows.1, Style::default().fg(if is_repo { ACCENT } else { DIM })),
    ]);
    frame.render_widget(Paragraph::new(repo_line), chunks[3]);

    // --- Branch toggle row ---
    let is_toggle = matches!(focus, NewAgentFocus::BranchToggle);
    let mode_label = match branch_mode {
        BranchMode::New => "New",
        BranchMode::Existing => "Existing",
    };
    let toggle_line = Line::from(vec![
        Span::styled("  Branch", label_style(is_toggle)),
        Span::raw(" ".repeat((label_w as usize).saturating_sub(8))),
        Span::styled("\u{2039} ", Style::default().fg(if is_toggle { ACCENT } else { DIM })),
        Span::styled(mode_label, val_style(is_toggle)),
        Span::styled(" \u{203a}", Style::default().fg(if is_toggle { ACCENT } else { DIM })),
    ]);
    frame.render_widget(Paragraph::new(toggle_line), chunks[5]);

    // --- Branch list ---
    let is_list = matches!(focus, NewAgentFocus::BranchList);
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
        let lines: Vec<Line> = active_list.iter()
            .enumerate()
            .skip(scroll)
            .take(visible)
            .map(|(i, b)| {
                let selected = i == *base_index;
                let indicator = if selected { "\u{2502} " } else { "  " };
                let style = if selected && is_list {
                    Style::default().fg(ACCENT)
                } else if selected {
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
                Span::styled("_", Style::default().fg(ACCENT)),
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
        let line_count: u16 = text.split('\n')
            .map(|l| if l.is_empty() { 1 } else { ((l.len() as u16).saturating_add(width as u16 - 1)) / width as u16 })
            .sum();
        let scroll = line_count.saturating_sub(prompt_area.height);
        let paragraph = Paragraph::new(text)
            .style(Style::default().fg(if is_prompt { ACCENT } else { TEXT }))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0));
        frame.render_widget(paragraph, prompt_area);
    }

    // --- Hint bar ---
    let hint = match focus {
        NewAgentFocus::Agent | NewAgentFocus::Repo | NewAgentFocus::BranchToggle => {
            "\u{2190} \u{2192} cycle \u{00b7} tab next \u{00b7} esc cancel"
        }
        NewAgentFocus::BranchList => {
            "\u{2191} \u{2193} select \u{00b7} tab next \u{00b7} esc cancel"
        }
        NewAgentFocus::Name => {
            "tab next \u{00b7} esc cancel"
        }
        NewAgentFocus::Prompt => {
            "enter start \u{00b7} alt+enter newline \u{00b7} tab options \u{00b7} esc cancel"
        }
    };
    let hint_line = Line::from(vec![
        Span::raw(" ".repeat(label_w as usize)),
        Span::styled(hint, Style::default().fg(DIM)),
    ]);
    frame.render_widget(Paragraph::new(hint_line), chunks[11]);
}

fn draw_delete_modal(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(" Delete Agent ", Style::default().fg(ACCENT)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let agent = app.selected_agent();
    let name = agent
        .map(|a| a.branch.as_str())
        .unwrap_or("?");
    let has_session = agent.is_some_and(|a| a.status.has_session());

    let chunks = Layout::vertical([
        Constraint::Length(1),  // top padding
        Constraint::Length(1),  // line 1
        Constraint::Length(1),  // line 2
        Constraint::Length(1),  // line 3
        Constraint::Min(0),    // spacer
        Constraint::Length(1),  // hint bar
    ])
    .split(inner);

    let msg1 = Line::from(Span::styled(
        "  Delete worktree and branch for",
        Style::default().fg(TEXT),
    ));
    let msg2 = Line::from(vec![
        Span::styled("  ", Style::default().fg(TEXT)),
        Span::styled(name, Style::default().fg(ACCENT)),
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
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("y", Style::default().fg(ACCENT)),
            Span::styled(" delete + tmux  ", Style::default().fg(DIM)),
            Span::styled("p", Style::default().fg(ACCENT)),
            Span::styled(" preserve tmux  ", Style::default().fg(DIM)),
            Span::styled("esc", Style::default().fg(ACCENT)),
            Span::styled(" cancel", Style::default().fg(DIM)),
        ])
    } else {
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("y", Style::default().fg(ACCENT)),
            Span::styled(" confirm  ", Style::default().fg(DIM)),
            Span::styled("esc", Style::default().fg(ACCENT)),
            Span::styled(" cancel", Style::default().fg(DIM)),
        ])
    };
    frame.render_widget(Paragraph::new(hint), chunks[5]);
}
