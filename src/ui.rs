use crate::agent_table::AgentTableWidget;
use crate::app::{App, Mode, MrSnapshot, PreviewMode};
use crate::gitlab::{MergeRequest, MrDisplayKind, MrState, classify};
use crate::new_agent_panel::NewAgentPanelWidget;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::style::{
    BUSY, DIM, FAIL, OK, TEXT, drift_arrow, footer_hint, modal_title, status_color,
};

const AGENT_TABLE_HEIGHT: u16 = 6;

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

    if matches!(app.mode, Mode::NewAgent { .. }) {
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
    let has_new_agent_candidate = matches!(app.mode, Mode::NewAgent { .. });
    let position_spans: Option<Vec<Span>> = if total > 0 || has_new_agent_candidate {
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
        if has_new_agent_candidate {
            if total > 0 {
                spans.push(Span::styled(" ", dim_style));
            }
            spans.push(Span::styled("\u{25E6}", dim_style));
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
    if let Mode::NewAgent { focus, .. } = &app.mode {
        let line = crate::new_agent_panel::wizard_hint(focus);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }
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
    frame.render_widget(Paragraph::new(Line::from(spans)), hint_bar);
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

    let hint = footer_hint(&[("y", "merge"), ("q/esc", "cancel")]);
    let mut spans = vec![Span::raw("  ")];
    spans.extend(hint.spans);
    frame.render_widget(Paragraph::new(Line::from(spans)), hint_bar);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, AgentStatus};
    use crate::app::Action;
    use crate::app::{BranchMode, Mode, NewAgentFocus, NewAgentSource, RemoteList};
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

    fn branch_source_app() -> App {
        let mut app = test_app();
        app.update(Action::StartNewAgent);
        if let Mode::NewAgent {
            source,
            focus,
            branch_mode,
            branches,
            branch_name,
            prompt,
            ..
        } = &mut app.mode
        {
            *source = NewAgentSource::Branch;
            *focus = NewAgentFocus::Source;
            *branch_mode = BranchMode::New;
            *branches = vec![
                "main".into(),
                "team/render-task-list".into(),
                "feat/configure-retry-env".into(),
                "team/system-version".into(),
                "search_strategy".into(),
                "fix/local-disk-pressure-cascade".into(),
            ];
            *branch_name = "z-0506-138-feature-task-wizard-layout-polish".into();
            prompt.clear();
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
            text.contains("Source   issue  mr  branch"),
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
        let source = text.find("Source   issue  mr  branch").expect(&text);
        let search = text.find("Search   filter issues...").expect(&text);
        let prompt = text.find("Prompt").expect(&text);
        let agent = text.find("Agent   claude  codex").expect(&text);
        assert!(
            repo < source && source < search && search < prompt && prompt < agent,
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
        if let Mode::NewAgent { prompt, .. } = &mut app.mode {
            *prompt = "Refine wizard layout behavior".into();
        }

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
        if let Mode::NewAgent {
            focus,
            name_pristine,
            ..
        } = &mut app.mode
        {
            *focus = NewAgentFocus::Name;
            *name_pristine = true;
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

        if let Mode::NewAgent {
            focus,
            issues,
            source_index,
            ..
        } = &mut app.mode
        {
            *focus = NewAgentFocus::SourceList;
            *source_index = 7;
            *issues =
                RemoteList::Loaded((1..=8).map(|n| issue(n, &format!("Issue {n}"))).collect());
        } else {
            panic!("expected new-agent mode");
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

        if let Mode::NewAgent {
            focus,
            source,
            mrs,
            source_index,
            selected_mr,
            ..
        } = &mut app.mode
        {
            *focus = NewAgentFocus::SourceList;
            *source = NewAgentSource::Mr;
            *source_index = 7;
            let items = (1..=8)
                .map(|n| mr(n, &format!("MR {n}"), &format!("feature/mr-{n}")))
                .collect::<Vec<_>>();
            *selected_mr = items.get(7).cloned();
            *mrs = RemoteList::Loaded(items);
        } else {
            panic!("expected new-agent mode");
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

        if let Mode::NewAgent {
            focus,
            source,
            mrs,
            source_index,
            selected_mr,
            ..
        } = &mut app.mode
        {
            *focus = NewAgentFocus::SourceList;
            *source = NewAgentSource::Mr;
            *source_index = 5;
            let items = (1..=7)
                .map(|n| mr(n, &format!("MR {n}"), &format!("feature/mr-{n}")))
                .collect::<Vec<_>>();
            *selected_mr = items.get(5).cloned();
            *mrs = RemoteList::Loaded(items);
        } else {
            panic!("expected new-agent mode");
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

        if let Mode::NewAgent {
            focus,
            source,
            branch_mode,
            branches,
            base_index,
            ..
        } = &mut app.mode
        {
            *focus = NewAgentFocus::BranchList;
            *source = NewAgentSource::Branch;
            *branch_mode = BranchMode::New;
            *base_index = 7;
            *branches = (1..=8).map(|n| format!("branch-{n}")).collect();
        } else {
            panic!("expected new-agent mode");
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
