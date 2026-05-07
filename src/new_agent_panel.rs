use crate::app::{App, BranchMode, Mode, NewAgentFocus, NewAgentSource, PromptMode, RemoteList};
use crate::gitlab::{GitlabIssue, GitlabMergeRequest};
use crate::style::{DIM, TEXT, footer_hint};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        StatefulWidget, Widget, Wrap,
    },
};

const NEW_AGENT_LABEL_W: u16 = 14;
const MAX_TASK_NAME_WIDTH: u16 = 40;
const MAX_SOURCE_LIST_WIDTH: u16 = 88;
const LABEL_W: u16 = NEW_AGENT_LABEL_W;
const PROMPT_BODY_HEIGHT: u16 = 3;

pub struct NewAgentPanelWidget<'a> {
    app: &'a App,
}

impl<'a> NewAgentPanelWidget<'a> {
    pub const fn new(app: &'a App) -> Self {
        Self { app }
    }
}

impl Widget for &NewAgentPanelWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if matches!(self.app.mode, Mode::NewAgent { .. }) {
            render_new_agent_panel(self.app, area, buf);
        }
    }
}

#[cfg(test)]
fn source_label(source: NewAgentSource) -> &'static str {
    match source {
        NewAgentSource::Issue => "issue",
        NewAgentSource::Mr => "mr",
        NewAgentSource::Branch => "branch",
    }
}

fn prompt_summary(
    source: NewAgentSource,
    prompt_mode: PromptMode,
    prompt: &str,
    max_width: u16,
) -> String {
    let summary = if matches!(prompt_mode, PromptMode::Custom) && !prompt.trim().is_empty() {
        prompt.lines().next().unwrap_or("").trim()
    } else if prompt.trim().is_empty() {
        "optional prompt"
    } else if matches!(prompt_mode, PromptMode::Custom) {
        "custom prompt"
    } else {
        match source {
            NewAgentSource::Issue => "generated from issue",
            NewAgentSource::Mr => "generated from MR",
            NewAgentSource::Branch => "optional prompt",
        }
    };
    truncate_end(summary, max_width as usize)
}

fn text_width(s: &str) -> usize {
    Span::raw(s).width()
}

fn take_prefix_width(s: &str, max_width: usize) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        let mut next = out.clone();
        next.push(ch);
        if text_width(&next) > max_width {
            break;
        }
        out = next;
    }
    out
}

fn take_suffix_width(s: &str, max_width: usize) -> String {
    let mut out = String::new();
    for ch in s.chars().rev() {
        let mut next = String::new();
        next.push(ch);
        next.push_str(&out);
        if text_width(&next) > max_width {
            break;
        }
        out = next;
    }
    out
}

fn truncate_end(s: &str, max_width: usize) -> String {
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

fn truncate_middle(s: &str, max_width: usize) -> String {
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

fn split_row(row: Rect) -> (Rect, Rect) {
    let [label, value] =
        Layout::horizontal([Constraint::Length(LABEL_W), Constraint::Min(0)]).areas(row);
    (label, value)
}

// `render_label` prepends a 2-col gutter so the label starts at column 2.
// This reserves space for the focus accent bar that Task 8 will draw via
// `Borders::LEFT` on the focused row's value sub-rect; until then, every row
// gets the same indent so widths stay constant. The prefix is also relied on
// by `ui.rs` snapshot tests (`new_agent_wizard_renders_source_tabs`,
// `new_agent_wizard_orders_primary_controls`,
// `new_agent_wizard_renders_prompt_and_agent_tabs`,
// `generated_prompt_is_collapsed_until_prompt_focus`) which assert literal
// strings like `"Repo        myapp"` and `"Source      issue  mr  branch"`.
// Task 8 must remove this `format!` and replace it with the bordered block,
// updating those snapshot tests in lockstep.
fn render_label(text: &str, focused: bool, area: Rect, buf: &mut Buffer) {
    let style = if focused {
        Style::default().fg(TEXT)
    } else {
        Style::default().fg(DIM)
    };
    Paragraph::new(Span::styled(format!("  {text}"), style)).render(area, buf);
}

fn render_value(text: &str, focused: bool, area: Rect, buf: &mut Buffer) {
    let style = if focused {
        Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(TEXT)
    };
    Paragraph::new(Span::styled(text.to_string(), style)).render(area, buf);
}

fn tab_value_line(options: &[&str], selected: usize) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, opt) in options.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", Style::default().fg(DIM)));
        }
        let style = if i == selected {
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };
        spans.push(Span::styled((*opt).to_string(), style));
    }
    Line::from(spans)
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
    count.clamp(1, 6) as u16
}

fn list_scroll_offset(selected_pos: Option<usize>, visible_rows: u16) -> usize {
    let visible = visible_rows as usize;
    selected_pos
        .filter(|_| visible > 0)
        .map(|pos| pos.saturating_add(1).saturating_sub(visible))
        .unwrap_or(0)
}

fn list_content_area(area: Rect, needs_scrollbar: bool) -> Rect {
    if needs_scrollbar && area.width > 1 {
        Rect {
            width: area.width.saturating_sub(1),
            ..area
        }
    } else {
        area
    }
}

fn list_outer_area(area: Rect) -> Rect {
    Rect {
        width: area.width.min(MAX_SOURCE_LIST_WIDTH),
        ..area
    }
}

fn render_selectable_list(
    lines: Vec<Line<'static>>,
    selected_pos: Option<usize>,
    area: Rect,
    buf: &mut Buffer,
) {
    Paragraph::new("").render(area, buf);
    let area = list_outer_area(area);
    let visible_rows = area.height;
    let offset = list_scroll_offset(selected_pos, visible_rows);
    let content_len = lines.len();
    let show_scrollbar = content_len > visible_rows as usize && area.width > 1;
    let list_area = list_content_area(area, show_scrollbar);

    let mut state = ListState::default()
        .with_selected(selected_pos)
        .with_offset(offset);
    let items = lines.into_iter().map(ListItem::new).collect::<Vec<_>>();
    let list = List::new(items).style(Style::default().fg(DIM));

    StatefulWidget::render(list, list_area, buf, &mut state);

    if show_scrollbar {
        let mut scrollbar_state = ScrollbarState::new(content_len)
            .viewport_content_length(visible_rows as usize)
            .position(state.offset());
        StatefulWidget::render(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("\u{2502}"))
                .thumb_symbol("\u{2590}")
                .track_style(Style::default().fg(DIM))
                .thumb_style(Style::default().fg(TEXT)),
            area,
            buf,
            &mut scrollbar_state,
        );
    }
}

fn render_new_agent_panel(app: &App, area: Rect, buf: &mut Buffer) {
    let inner = area;

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
        prompt_mode,
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
    let label_w = LABEL_W;
    let desired_list_height = source_list_height(*source, issues, mrs, active_list, source_query);
    let show_gitlab_source = matches!(source, NewAgentSource::Issue | NewAgentSource::Mr);
    let show_branch_controls = matches!(source, NewAgentSource::Branch | NewAgentSource::Issue);
    let show_branch_toggle = matches!(source, NewAgentSource::Branch);
    let show_name = show_branch_controls
        && matches!(branch_mode, BranchMode::New)
        && !matches!(source, NewAgentSource::Issue);
    let show_issue_name = matches!(source, NewAgentSource::Issue);
    let show_name_row = show_name || show_issue_name;
    let is_prompt = matches!(focus, NewAgentFocus::Prompt);
    let list_height = desired_list_height.clamp(1, 6);

    // Row order. `Length(0)` rows render nothing and consume nothing visually.
    // Dividers reserve zero height in this task; Task 9 will switch them to
    // `Length(1)` once their `─` glyph is painted. The list row uses `Max` so
    // it shrinks gracefully in tight viewports. The trailing `Min(0)` is the
    // single explicit slack absorber — every other row carries an exact size,
    // so a future `Min`/`Max` constraint cannot accidentally split the slack.
    let constraints = [
        Constraint::Length(1),                                      // 0  Repo
        Constraint::Length(1),                                      // 1  Source
        Constraint::Length(if show_branch_toggle { 1 } else { 0 }), // 2  Branch toggle
        Constraint::Length(if show_gitlab_source { 1 } else { 0 }), // 3  Search
        Constraint::Max(list_height),                               // 4  List
        Constraint::Length(0),                                      // 5  Divider 1
        Constraint::Length(if show_name_row { 1 } else { 0 }),      // 6  Name
        Constraint::Length(1),                                      // 7  Prompt label
        Constraint::Length(PROMPT_BODY_HEIGHT),                     // 8  Prompt body
        Constraint::Length(0),                                      // 9  Divider 2
        Constraint::Length(1),                                      // 10 Agent
        Constraint::Length(0),                                      // 11 Divider 3
        Constraint::Length(1),                                      // 12 Hint
        Constraint::Min(0),                                         // 13 Trailing slack
    ];

    let chunks = Layout::vertical(constraints)
        .flex(Flex::Start)
        .spacing(0)
        .split(inner);

    // --- Repo row ---
    let is_repo = matches!(focus, NewAgentFocus::Repo);
    let (repo_label_rect, repo_value_rect) = split_row(chunks[0]);
    render_label("Repo", is_repo, repo_label_rect, buf);
    render_value(repo_name, is_repo, repo_value_rect, buf);

    // --- Source row ---
    let is_source = matches!(focus, NewAgentFocus::Source);
    let source_selected = match source {
        NewAgentSource::Issue => 0,
        NewAgentSource::Mr => 1,
        NewAgentSource::Branch => 2,
    };
    let (source_label_rect, source_value_rect) = split_row(chunks[1]);
    render_label("Source", is_source, source_label_rect, buf);
    Paragraph::new(tab_value_line(
        &["issue", "mr", "branch"],
        source_selected,
    ))
    .render(source_value_rect, buf);

    // --- Branch toggle row ---
    if show_branch_toggle {
        let is_toggle = matches!(focus, NewAgentFocus::BranchToggle);
        let mode_label = match branch_mode {
            BranchMode::New => "New",
            BranchMode::Existing => "Existing",
        };
        let (toggle_label_rect, toggle_value_rect) = split_row(chunks[2]);
        render_label("Branch", is_toggle, toggle_label_rect, buf);
        render_value(mode_label, is_toggle, toggle_value_rect, buf);
    }

    // --- Source or branch list ---
    let list_area = chunks[4];
    if show_gitlab_source {
        let is_search = matches!(focus, NewAgentFocus::Search);
        let (search_label_rect, search_value_rect) = split_row(chunks[3]);
        render_label("Search", is_search, search_label_rect, buf);
        let (search_value_text, search_value_style) = if source_query.is_empty() {
            let placeholder = match source {
                NewAgentSource::Issue => "filter issues...",
                NewAgentSource::Mr => "filter MRs...",
                NewAgentSource::Branch => "",
            };
            (placeholder.to_string(), Style::default().fg(DIM))
        } else {
            let style = if is_search {
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT)
            };
            (source_query.clone(), style)
        };
        Paragraph::new(Span::styled(search_value_text, search_value_style))
            .render(search_value_rect, buf);

        let all_lines = match source {
            NewAgentSource::Issue => {
                filtered_issue_lines(issues, source_query, *source_index, label_w)
            }
            NewAgentSource::Mr => filtered_mr_lines(mrs, source_query, *source_index, label_w),
            NewAgentSource::Branch => Vec::new(),
        };
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
        render_selectable_list(all_lines, selected_pos, list_area, buf);
    } else if active_list.is_empty() {
        let empty_msg = match branch_mode {
            BranchMode::New => "loading...",
            BranchMode::Existing => "no existing branches",
        };
        Paragraph::new(remote_status_line(empty_msg, label_w)).render(list_area, buf);
    } else {
        let lines = active_list
            .iter()
            .enumerate()
            .map(|(i, b)| selectable_source_line(b.clone(), i == *base_index, label_w))
            .collect();
        render_selectable_list(lines, Some(*base_index), list_area, buf);
    }

    // --- Name row ---
    let name_value_width = chunks[6]
        .width
        .saturating_sub(label_w)
        .max(1)
        .min(MAX_TASK_NAME_WIDTH) as usize;
    if show_name {
        let is_name = matches!(focus, NewAgentFocus::Name);
        let (name_label_rect, name_value_rect) = split_row(chunks[6]);
        render_label("Name", is_name, name_label_rect, buf);
        let name_display = if is_name && *name_pristine {
            // Pristine auto-suggested name: dim + italic so it reads as a
            // placeholder that will be replaced the moment the user types.
            let name = truncate_middle(branch_name, name_value_width);
            Span::styled(
                name,
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            )
        } else {
            let cursor = if is_name { "_" } else { "" };
            let max_width = name_value_width.saturating_sub(cursor.len());
            let name = truncate_middle(branch_name, max_width);
            let style = if is_name {
                Style::default().fg(TEXT)
            } else {
                Style::default().fg(DIM)
            };
            Span::styled(format!("{name}{cursor}"), style)
        };
        Paragraph::new(Line::from(name_display)).render(name_value_rect, buf);
    } else if show_issue_name {
        let (name_label_rect, name_value_rect) = split_row(chunks[6]);
        render_label("Name", false, name_label_rect, buf);
        let name = truncate_middle(branch_name, name_value_width);
        Paragraph::new(Span::styled(name, Style::default().fg(TEXT)))
            .render(name_value_rect, buf);
    }

    // --- Prompt tabs ---
    let prompt_selected = match prompt_mode {
        PromptMode::Generated => 0,
        PromptMode::Custom => 1,
    };
    let (prompt_label_rect, prompt_value_rect) = split_row(chunks[7]);
    render_label("Prompt", is_prompt, prompt_label_rect, buf);
    Paragraph::new(tab_value_line(&["default", "custom"], prompt_selected))
        .render(prompt_value_rect, buf);

    // --- Prompt area ---
    // chunks[8] is exactly `PROMPT_BODY_HEIGHT` rows; trailing slack is
    // absorbed by the synthetic `Min(0)` row at the bottom of the layout.
    let (_label, body_rect) = split_row(chunks[8]);
    if !is_prompt {
        let summary = prompt_summary(*source, *prompt_mode, prompt, body_rect.width);
        Paragraph::new(Span::styled(summary, Style::default().fg(DIM)))
            .render(body_rect, buf);
    } else if prompt.is_empty() {
        let placeholder = Span::styled("_", Style::default().fg(TEXT));
        Paragraph::new(placeholder).render(body_rect, buf);
    } else {
        let cursor = "_";
        let text = format!("{prompt}{cursor}");
        let width = body_rect.width.max(1) as usize;
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
        let scroll = line_count.saturating_sub(body_rect.height);
        Paragraph::new(text)
            .style(Style::default().fg(TEXT))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .render(body_rect, buf);
    }

    // --- Agent tabs ---
    let is_agent = matches!(focus, NewAgentFocus::Agent);
    let agent_options: Vec<&str> = app
        .config
        .agents
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let agent_selected = agent_options
        .iter()
        .position(|name| *name == agent_name)
        .unwrap_or(0);
    let (agent_label_rect, agent_value_rect) = split_row(chunks[10]);
    render_label("Agent", is_agent, agent_label_rect, buf);
    Paragraph::new(tab_value_line(&agent_options, agent_selected))
        .render(agent_value_rect, buf);

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
    Paragraph::new(Line::from(spans)).render(chunks[12], buf);
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn list_scroll_offset_keeps_selected_last_visible() {
        assert_eq!(list_scroll_offset(Some(0), 6), 0);
        assert_eq!(list_scroll_offset(Some(5), 6), 0);
        assert_eq!(list_scroll_offset(Some(6), 6), 1);
        assert_eq!(list_scroll_offset(Some(7), 6), 2);
        assert_eq!(list_scroll_offset(None, 6), 0);
        assert_eq!(list_scroll_offset(Some(7), 0), 0);
    }

    #[test]
    fn list_content_area_reserves_scrollbar_column_only_when_needed() {
        let area = Rect::new(2, 3, 20, 6);

        assert_eq!(list_content_area(area, false), area);
        assert_eq!(list_content_area(area, true), Rect::new(2, 3, 19, 6));
        assert_eq!(
            list_content_area(Rect::new(0, 0, 1, 6), true),
            Rect::new(0, 0, 1, 6)
        );
    }

    #[test]
    fn render_selectable_list_hides_scrollbar_when_rows_fill_viewport() {
        let area = Rect::new(0, 0, 80, 6);
        let mut buf = Buffer::empty(area);
        let lines = (1..=6)
            .map(|n| Line::from(format!("item {n}")))
            .collect::<Vec<_>>();

        render_selectable_list(lines, Some(0), area, &mut buf);

        assert!(
            !buf.content().iter().any(|cell| cell.symbol() == "\u{2590}"),
            "filled viewport with no hidden rows should not show a scrollbar"
        );
    }

    #[test]
    fn render_selectable_list_shows_scrollbar_when_rows_exceed_viewport() {
        let area = Rect::new(0, 0, 80, 6);
        let mut buf = Buffer::empty(area);
        let lines = (1..=7)
            .map(|n| Line::from(format!("item {n}")))
            .collect::<Vec<_>>();

        render_selectable_list(lines, Some(6), area, &mut buf);

        assert!(
            buf.content().iter().any(|cell| cell.symbol() == "\u{2590}"),
            "hidden rows should show a list scrollbar"
        );
    }

    #[test]
    fn list_outer_area_caps_wide_source_lists() {
        assert_eq!(
            list_outer_area(Rect::new(2, 3, 200, 6)),
            Rect::new(2, 3, MAX_SOURCE_LIST_WIDTH, 6)
        );
        assert_eq!(
            list_outer_area(Rect::new(2, 3, 80, 6)),
            Rect::new(2, 3, 80, 6)
        );
    }

    /// Build a minimal `App` whose `mode` is `Mode::NewAgent { .. }`.
    ///
    /// Mirrors `app::tests::test_app_in_new_agent_mode`, but kept self-contained
    /// here because that helper lives in a private `#[cfg(test)] mod tests`
    /// inside `app.rs` and isn't reachable from another module's tests.
    fn wizard_app() -> App {
        let toml_str = r#"repos = ["~/src/myapp"]"#;
        let config = crate::config::Config::from_toml_str(toml_str).unwrap();
        let mut app = App::new(config);
        app.mode = Mode::NewAgent {
            repo_index: 0,
            source: NewAgentSource::Branch,
            source_query: String::new(),
            source_index: 0,
            issues: RemoteList::Idle,
            mrs: RemoteList::Idle,
            selected_issue: None,
            selected_mr: None,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            prompt_mode: PromptMode::Custom,
            focus: NewAgentFocus::Prompt,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        app
    }

    fn render_with_focus(focus: NewAgentFocus, area: Rect) -> Buffer {
        // `App` isn't `Clone` (Config and several inner types don't derive it),
        // so build a fresh wizard app per call instead of cloning.
        let mut app = wizard_app();
        if let Mode::NewAgent { focus: f, .. } = &mut app.mode {
            *f = focus;
        }
        let mut buf = Buffer::empty(area);
        NewAgentPanelWidget::new(&app).render(area, &mut buf);
        buf
    }

    fn cells_outside_columns(buf: &Buffer, skip_cols: std::ops::Range<u16>) -> Vec<(u16, u16, String)> {
        let mut out = Vec::new();
        let area = *buf.area();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if skip_cols.contains(&x) {
                    continue;
                }
                let cell = &buf[(x, y)];
                out.push((x, y, cell.symbol().to_string()));
            }
        }
        out
    }

    #[test]
    fn focus_changes_do_not_move_form_rows() {
        let area = Rect::new(0, 0, 80, 24);

        let on_repo = render_with_focus(NewAgentFocus::Repo, area);
        let on_prompt = render_with_focus(NewAgentFocus::Prompt, area);

        // The focus accent bar plus the value column may legitimately differ.
        // The label column (columns 0..LABEL_W) must be identical, because every
        // row's label is invariant — if rows shift vertically, label glyphs land
        // at different y between the two states.
        let a = cells_outside_columns(&on_repo, NEW_AGENT_LABEL_W..area.width);
        let b = cells_outside_columns(&on_prompt, NEW_AGENT_LABEL_W..area.width);

        assert_eq!(
            a, b,
            "form rows shifted between focus states; geometry must be fixed"
        );
    }

    #[test]
    fn collapsed_prompt_summary_renders_in_first_body_row_only() {
        let mut app = wizard_app();
        if let Mode::NewAgent {
            focus,
            prompt,
            prompt_mode,
            ..
        } = &mut app.mode
        {
            *focus = NewAgentFocus::Repo;
            *prompt = "describe the work".to_string();
            *prompt_mode = PromptMode::Custom;
        }
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        NewAgentPanelWidget::new(&app).render(area, &mut buf);

        // Find the prompt body's first row by scanning for the summary text.
        let mut summary_row = None;
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(buf[(x, y)].symbol());
            }
            if line.contains("describe the work") {
                summary_row = Some(y);
                break;
            }
        }
        let row = summary_row.expect("prompt summary row not rendered");

        // The two rows below the summary must be blank.
        for dy in 1..=2 {
            let y = row + dy;
            for x in 0..area.width {
                assert_eq!(
                    buf[(x, y)].symbol(),
                    " ",
                    "expected blank cell at ({x},{y}); body rows past row 1 must be empty"
                );
            }
        }
    }

    #[test]
    fn focused_prompt_wrapped_lines_align_to_value_column() {
        let mut app = wizard_app();
        if let Mode::NewAgent {
            focus,
            prompt,
            prompt_mode,
            ..
        } = &mut app.mode
        {
            *focus = NewAgentFocus::Prompt;
            *prompt = "a ".repeat(60);
            *prompt_mode = PromptMode::Custom;
        }
        let area = Rect::new(0, 0, 50, 24);
        let mut buf = Buffer::empty(area);
        NewAgentPanelWidget::new(&app).render(area, &mut buf);

        // Find the rows containing the prompt content (start with 'a').
        let mut content_rows: Vec<u16> = Vec::new();
        for y in 0..area.height {
            let cell = buf[(LABEL_W, y)].symbol();
            if cell == "a" {
                content_rows.push(y);
            }
        }
        assert!(
            content_rows.len() >= 2,
            "expected wrapped prompt to occupy multiple rows; saw {} rows",
            content_rows.len()
        );

        // Every content row must start with 'a' at column LABEL_W (value column),
        // and column LABEL_W - 1 (label column) must be blank.
        for y in content_rows {
            assert_eq!(buf[(LABEL_W, y)].symbol(), "a", "value column drift at y={y}");
            assert_eq!(buf[(LABEL_W - 1, y)].symbol(), " ", "label column non-blank at y={y}");
        }
    }
}
