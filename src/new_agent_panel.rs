use crate::app::{App, BranchMode, Mode, NewAgentFocus, NewAgentSource, RemoteList};
use crate::gitlab::{GitlabIssue, GitlabMergeRequest};
use crate::style::{DIM, TEXT, footer_hint};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, List, ListItem, ListState, Padding, Paragraph,
        StatefulWidget, Widget, Wrap,
    },
};

const LABEL_W: u16 = 14;
const MAX_TASK_NAME_WIDTH: u16 = 40;
const PROMPT_BODY_HEIGHT: u16 = 3;

/// Payload prepared by `*_items` helpers, consumed by the stock `List` render.
///
/// `Status` routes through `render_remote_status` (loading / failed / empty).
/// `Items` carries owned label strings plus the originating `RemoteList` index
/// for each row, so the caller can map a `ListState` selection back to the
/// `source_index` field on `Mode::NewAgent`.
enum ListPayload {
    Status(String),
    Items {
        labels: Vec<String>,
        indices: Vec<usize>,
    },
}

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

fn prompt_summary(prompt: &str, max_width: u16) -> String {
    let summary = if !prompt.trim().is_empty() {
        prompt.lines().next().unwrap_or("").trim()
    } else {
        "optional prompt"
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

fn render_label(text: &str, focused: bool, area: Rect, buf: &mut Buffer) {
    let style = if focused {
        Style::default().fg(TEXT)
    } else {
        Style::default().fg(DIM)
    };
    Paragraph::new(Span::styled(text.to_string(), style))
        .alignment(Alignment::Right)
        .block(Block::new().padding(Padding::right(1)))
        .render(area, buf);
}

/// Wraps a row's value sub-rect with a focus accent bar on focus, or a 2-col
/// left padding when unfocused, so content lands at the same column either way.
fn focus_block(focused: bool) -> Block<'static> {
    if focused {
        Block::new()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(TEXT))
            .padding(Padding::horizontal(1))
    } else {
        Block::new().padding(Padding::left(2))
    }
}

/// Renders the focus frame (accent bar or left padding) into `area` and
/// returns the inner rect callers should draw their content into. Centralizes
/// the three-step `block.inner` / `block.render` ritual so a new row can't
/// silently drop the accent bar by forgetting one of the steps.
fn render_focus_frame(focused: bool, area: Rect, buf: &mut Buffer) -> Rect {
    let block = focus_block(focused);
    let inner = block.inner(area);
    block.render(area, buf);
    inner
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

fn render_divider(area: Rect, buf: &mut Buffer) {
    Block::new()
        .borders(Borders::TOP)
        .border_type(BorderType::LightTripleDashed)
        .border_style(Style::default().fg(DIM))
        .render(area, buf);
}

fn render_remote_status(message: &str, area: Rect, buf: &mut Buffer) {
    Paragraph::new(Span::styled(message.to_string(), Style::default().fg(DIM))).render(area, buf);
}

fn matches_source_query(label: &str, query: &str) -> bool {
    let trimmed = query.trim();
    trimmed.is_empty()
        || label
            .to_ascii_lowercase()
            .contains(&trimmed.to_ascii_lowercase())
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

fn issue_items(issues: &RemoteList<GitlabIssue>, query: &str) -> ListPayload {
    match issues {
        RemoteList::Idle | RemoteList::Loading => {
            ListPayload::Status("loading assigned issues...".to_string())
        }
        RemoteList::Failed(message) => ListPayload::Status(format!("error: {message}")),
        RemoteList::Loaded(items) => {
            let indices = filtered_issue_indices(items, query);
            if indices.is_empty() {
                let msg = if items.is_empty() {
                    "no assigned issues"
                } else {
                    "no matching issues"
                };
                return ListPayload::Status(msg.to_string());
            }
            let labels: Vec<String> = indices.iter().map(|i| issue_label(&items[*i])).collect();
            ListPayload::Items { labels, indices }
        }
    }
}

fn mr_items(mrs: &RemoteList<GitlabMergeRequest>, query: &str) -> ListPayload {
    match mrs {
        RemoteList::Idle | RemoteList::Loading => {
            ListPayload::Status("loading review MRs...".to_string())
        }
        RemoteList::Failed(message) => ListPayload::Status(format!("error: {message}")),
        RemoteList::Loaded(items) => {
            let indices = filtered_mr_indices(items, query);
            if indices.is_empty() {
                let msg = if items.is_empty() {
                    "no MRs needing review"
                } else {
                    "no matching MRs"
                };
                return ListPayload::Status(msg.to_string());
            }
            let labels: Vec<String> = indices.iter().map(|i| mr_label(&items[*i])).collect();
            ListPayload::Items { labels, indices }
        }
    }
}

fn branch_items(branches: &[String], branch_mode: &BranchMode) -> ListPayload {
    if branches.is_empty() {
        return match branch_mode {
            BranchMode::New => ListPayload::Status("loading...".to_string()),
            BranchMode::Existing => ListPayload::Status("no existing branches".to_string()),
        };
    }
    let labels: Vec<String> = branches.to_vec();
    let indices: Vec<usize> = (0..branches.len()).collect();
    ListPayload::Items { labels, indices }
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
    // Dividers carry one row each and render a thin `─` rule between groups.
    // The list row uses `Max` so it shrinks gracefully in tight viewports. The
    // trailing `Min(0)` is the single explicit slack absorber — every other
    // row carries an exact size, so a future `Min`/`Max` constraint cannot
    // accidentally split the slack.
    let constraints = [
        Constraint::Length(1),                                      // 0  Repo
        Constraint::Length(1),                                      // 1  Source
        Constraint::Length(if show_branch_toggle { 1 } else { 0 }), // 2  Branch toggle
        Constraint::Length(if show_gitlab_source { 1 } else { 0 }), // 3  Search
        Constraint::Length(list_height),                            // 4  List
        Constraint::Length(1),                                      // 5  Divider 1
        Constraint::Length(if show_name_row { 1 } else { 0 }),      // 6  Name
        Constraint::Length(PROMPT_BODY_HEIGHT),                     // 7  Prompt (label + body)
        Constraint::Length(1),                                      // 8  Divider 2
        Constraint::Length(1),                                      // 9  Agent
        Constraint::Min(0),                                         // 10 Trailing slack
    ];

    let layout = Layout::vertical(constraints).flex(Flex::Start).spacing(0);
    let [
        repo_row,
        source_row,
        branch_toggle_row,
        search_row,
        list_area,
        divider_1_row,
        name_row,
        prompt_row,
        divider_2_row,
        agent_row,
        _trailing_slack,
    ] = inner.layout(&layout);

    // --- Repo row ---
    let is_repo = matches!(focus, NewAgentFocus::Repo);
    let (repo_label_rect, repo_value_rect) = split_row(repo_row);
    render_label("Repo", is_repo, repo_label_rect, buf);
    let repo_inner = render_focus_frame(is_repo, repo_value_rect, buf);
    render_value(repo_name, is_repo, repo_inner, buf);

    // --- Source row ---
    let is_source = matches!(focus, NewAgentFocus::Source);
    let source_selected = match source {
        NewAgentSource::Issue => 0,
        NewAgentSource::Mr => 1,
        NewAgentSource::Branch => 2,
    };
    let (source_label_rect, source_value_rect) = split_row(source_row);
    render_label("Source", is_source, source_label_rect, buf);
    let source_inner = render_focus_frame(is_source, source_value_rect, buf);
    Paragraph::new(tab_value_line(&["issue", "mr", "branch"], source_selected))
        .render(source_inner, buf);

    // --- Branch toggle row ---
    if show_branch_toggle {
        let is_toggle = matches!(focus, NewAgentFocus::BranchToggle);
        let toggle_selected = match branch_mode {
            BranchMode::New => 0,
            BranchMode::Existing => 1,
        };
        let (toggle_label_rect, toggle_value_rect) = split_row(branch_toggle_row);
        render_label("Branch", is_toggle, toggle_label_rect, buf);
        let toggle_inner = render_focus_frame(is_toggle, toggle_value_rect, buf);
        Paragraph::new(tab_value_line(&["new", "existing"], toggle_selected))
            .render(toggle_inner, buf);
    }

    // --- Source or branch list ---
    if show_gitlab_source {
        let is_search = matches!(focus, NewAgentFocus::Search);
        let (search_label_rect, search_value_rect) = split_row(search_row);
        render_label("Search", is_search, search_label_rect, buf);
        let (search_value_text, search_value_style) = if source_query.is_empty() {
            let placeholder = match source {
                NewAgentSource::Issue => "filter issues...",
                NewAgentSource::Mr => "filter MRs...",
                NewAgentSource::Branch => "",
            };
            (placeholder.to_string(), Style::default().fg(DIM))
        } else {
            (source_query.clone(), Style::default().fg(TEXT))
        };
        let search_inner = render_focus_frame(is_search, search_value_rect, buf);
        Paragraph::new(Span::styled(search_value_text, search_value_style))
            .render(search_inner, buf);
    }

    let (_l, list_value) = split_row(list_area);
    let list_focused = matches!(focus, NewAgentFocus::SourceList | NewAgentFocus::BranchList);
    let list_inner = render_focus_frame(list_focused, list_value, buf);
    let payload = match source {
        NewAgentSource::Issue => issue_items(issues, source_query),
        NewAgentSource::Mr => mr_items(mrs, source_query),
        NewAgentSource::Branch => branch_items(active_list, branch_mode),
    };
    match payload {
        ListPayload::Status(msg) => render_remote_status(&msg, list_inner, buf),
        ListPayload::Items { labels, indices } => {
            let target_index = match source {
                NewAgentSource::Issue | NewAgentSource::Mr => *source_index,
                NewAgentSource::Branch => *base_index,
            };
            let selected_pos = indices.iter().position(|&i| i == target_index);
            let items: Vec<ListItem> = labels.into_iter().map(ListItem::new).collect();
            let list = List::new(items)
                .style(Style::default().fg(DIM))
                .highlight_style(Style::default().fg(TEXT).add_modifier(Modifier::BOLD));
            let mut state = ListState::default().with_selected(selected_pos);
            StatefulWidget::render(list, list_inner, buf, &mut state);
        }
    }

    // --- Name row ---
    if show_name {
        let is_name = matches!(focus, NewAgentFocus::Name);
        let (name_label_rect, name_value_rect) = split_row(name_row);
        render_label("Name", is_name, name_label_rect, buf);
        let name_inner = render_focus_frame(is_name, name_value_rect, buf);
        let name_value_width = (name_inner.width as usize)
            .max(1)
            .min(MAX_TASK_NAME_WIDTH as usize);
        let name = truncate_middle(branch_name, name_value_width);
        let style = if *name_pristine {
            Style::default().fg(DIM)
        } else {
            Style::default().fg(TEXT)
        };
        Paragraph::new(Span::styled(name, style)).render(name_inner, buf);
    } else if show_issue_name {
        let (name_label_rect, name_value_rect) = split_row(name_row);
        render_label("Name", false, name_label_rect, buf);
        let name_inner = render_focus_frame(false, name_value_rect, buf);
        let name_value_width = (name_inner.width as usize)
            .max(1)
            .min(MAX_TASK_NAME_WIDTH as usize);
        let name = truncate_middle(branch_name, name_value_width);
        Paragraph::new(Span::styled(name, Style::default().fg(TEXT))).render(name_inner, buf);
    }

    // --- Prompt (label + body share one PROMPT_BODY_HEIGHT row so the label
    // top-aligns with the first body line). Label paragraph occupies the full
    // label column rect; only the first row holds glyphs.
    let (prompt_label_rect, body_rect) = split_row(prompt_row);
    render_label("Prompt", is_prompt, prompt_label_rect, buf);
    let body_inner = render_focus_frame(is_prompt, body_rect, buf);
    if !is_prompt {
        let summary = prompt_summary(prompt, body_inner.width);
        Paragraph::new(Span::styled(summary, Style::default().fg(DIM))).render(body_inner, buf);
    } else {
        let text = prompt.as_str();
        let width = body_inner.width.max(1) as usize;
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
        let scroll = line_count.saturating_sub(body_inner.height);
        Paragraph::new(text)
            .style(Style::default().fg(TEXT))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .render(body_inner, buf);
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
    let (agent_label_rect, agent_value_rect) = split_row(agent_row);
    render_label("Agent", is_agent, agent_label_rect, buf);
    let agent_inner = render_focus_frame(is_agent, agent_value_rect, buf);
    Paragraph::new(tab_value_line(&agent_options, agent_selected)).render(agent_inner, buf);

    // --- Group dividers ---
    render_divider(divider_1_row, buf);
    render_divider(divider_2_row, buf);
}

/// Per-focus hint line for the wizard, surfaced by the global status bar
/// while `Mode::NewAgent` is active.
pub(crate) fn wizard_hint(focus: &NewAgentFocus) -> Line<'static> {
    match focus {
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
        NewAgentFocus::Prompt => {
            footer_hint(&[("enter", "start"), ("e", "edit"), ("esc", "cancel")])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn issue_items_yield_status_when_loading() {
        let payload = issue_items(&RemoteList::Loading, "");
        assert!(matches!(payload, ListPayload::Status(_)));
    }

    #[test]
    fn issue_items_filter_by_query() {
        let issues = RemoteList::Loaded(vec![issue(1, "alpha"), issue(2, "beta")]);
        let payload = issue_items(&issues, "alp");
        match payload {
            ListPayload::Items { labels, indices } => {
                assert_eq!(labels, vec!["#1 alpha"]);
                assert_eq!(indices, vec![0]);
            }
            _ => panic!("expected Items"),
        }
    }

    #[test]
    fn issue_items_distinguish_empty_from_no_matches() {
        let empty = issue_items(&RemoteList::Loaded(vec![]), "");
        match empty {
            ListPayload::Status(msg) => assert_eq!(msg, "no assigned issues"),
            _ => panic!("expected Status"),
        }
        let issues = RemoteList::Loaded(vec![issue(1, "alpha")]);
        let no_match = issue_items(&issues, "billing");
        match no_match {
            ListPayload::Status(msg) => assert_eq!(msg, "no matching issues"),
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn mr_items_filter_by_query_and_include_source_branch() {
        let mrs = RemoteList::Loaded(vec![
            mr(7, "Review renderer", "feature/render"),
            mr(8, "Update docs", "docs/readme"),
        ]);
        let payload = mr_items(&mrs, "render");
        match payload {
            ListPayload::Items { labels, indices } => {
                assert_eq!(labels, vec!["!7 Review renderer feature/render"]);
                assert_eq!(indices, vec![0]);
            }
            _ => panic!("expected Items"),
        }
    }

    #[test]
    fn mr_items_distinguish_empty_from_no_matches() {
        let empty = mr_items(&RemoteList::Loaded(vec![]), "");
        match empty {
            ListPayload::Status(msg) => assert_eq!(msg, "no MRs needing review"),
            _ => panic!("expected Status"),
        }
        let mrs = RemoteList::Loaded(vec![mr(7, "Review renderer", "feature/render")]);
        let no_match = mr_items(&mrs, "billing");
        match no_match {
            ListPayload::Status(msg) => assert_eq!(msg, "no matching MRs"),
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn list_selection_change_keeps_items_at_same_column() {
        let mut app = wizard_app();
        if let Mode::NewAgent {
            source,
            issues,
            source_index,
            focus,
            ..
        } = &mut app.mode
        {
            *source = NewAgentSource::Issue;
            *issues =
                RemoteList::Loaded(vec![issue(1, "alpha"), issue(2, "beta"), issue(3, "gamma")]);
            *source_index = 0;
            *focus = NewAgentFocus::SourceList;
        }
        let area = Rect::new(0, 0, 80, 24);
        let mut buf_a = Buffer::empty(area);
        NewAgentPanelWidget::new(&app).render(area, &mut buf_a);

        if let Mode::NewAgent { source_index, .. } = &mut app.mode {
            *source_index = 1;
        }
        let mut buf_b = Buffer::empty(area);
        NewAgentPanelWidget::new(&app).render(area, &mut buf_b);

        // The text "alpha" must appear at the same column in both buffers.
        // Scan column-by-column (not by byte) because cell symbols can be
        // multi-byte (the `▌` highlight glyph is 3 bytes), and we want the
        // visual column where the substring begins.
        let col_in = |buf: &Buffer, needle: &str| -> Option<u16> {
            let needle_chars: Vec<char> = needle.chars().collect();
            for y in 0..area.height {
                let mut start_x: Option<u16> = None;
                let mut matched = 0usize;
                for x in 0..area.width {
                    let sym = buf[(x, y)].symbol();
                    if sym.chars().count() == 1 && sym.starts_with(needle_chars[matched]) {
                        if matched == 0 {
                            start_x = Some(x);
                        }
                        matched += 1;
                        if matched == needle_chars.len() {
                            return start_x;
                        }
                    } else {
                        matched = 0;
                        start_x = None;
                    }
                }
            }
            None
        };
        let col_a = col_in(&buf_a, "alpha").unwrap();
        let col_b = col_in(&buf_b, "alpha").unwrap();
        assert_eq!(
            col_a, col_b,
            "list item shifted horizontally when selection changed"
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

    fn cells_outside_columns(
        buf: &Buffer,
        skip_cols: std::ops::Range<u16>,
    ) -> Vec<(u16, u16, String)> {
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
        let a = cells_outside_columns(&on_repo, LABEL_W..area.width);
        let b = cells_outside_columns(&on_prompt, LABEL_W..area.width);

        assert_eq!(
            a, b,
            "form rows shifted between focus states; geometry must be fixed"
        );
    }

    #[test]
    fn collapsed_prompt_summary_renders_in_first_body_row_only() {
        let mut app = wizard_app();
        if let Mode::NewAgent { focus, prompt, .. } = &mut app.mode {
            *focus = NewAgentFocus::Repo;
            *prompt = "describe the work".to_string();
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
                let col = line.find("describe the work").unwrap() as u16;
                assert_eq!(col, LABEL_W + 2, "prompt summary column drift");
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
    fn focused_empty_prompt_does_not_render_inline_cursor() {
        let app = wizard_app();
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        NewAgentPanelWidget::new(&app).render(area, &mut buf);

        for y in 0..area.height {
            for x in 0..area.width {
                assert_ne!(
                    buf[(x, y)].symbol(),
                    "_",
                    "prompt should be edited through $EDITOR, not an inline cursor"
                );
            }
        }
    }

    #[test]
    fn two_group_dividers_render_light_triple_dashed_rules() {
        let app = wizard_app();
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        NewAgentPanelWidget::new(&app).render(area, &mut buf);

        let mut divider_rows = 0;
        for y in 0..area.height {
            let row: String = (0..area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            if row.chars().filter(|c| *c == '┄').count() >= (area.width as usize - 4) {
                divider_rows += 1;
            }
        }
        assert_eq!(
            divider_rows, 2,
            "expected exactly two light triple dashed group dividers"
        );
    }

    #[test]
    fn focused_prompt_wrapped_lines_align_to_value_column() {
        let mut app = wizard_app();
        if let Mode::NewAgent { focus, prompt, .. } = &mut app.mode {
            *focus = NewAgentFocus::Prompt;
            *prompt = "a ".repeat(60);
        }
        let area = Rect::new(0, 0, 50, 24);
        let mut buf = Buffer::empty(area);
        NewAgentPanelWidget::new(&app).render(area, &mut buf);

        // Find the rows containing the prompt content (start with 'a' at the
        // value column). With the focus accent bar, focused rows render the
        // `│` glyph at LABEL_W, a padding column at LABEL_W + 1, then content
        // at LABEL_W + 2.
        let mut content_rows: Vec<u16> = Vec::new();
        for y in 0..area.height {
            let cell = buf[(LABEL_W + 2, y)].symbol();
            if cell == "a" {
                content_rows.push(y);
            }
        }
        assert!(
            content_rows.len() >= 2,
            "expected wrapped prompt to occupy multiple rows; saw {} rows",
            content_rows.len()
        );

        for y in content_rows {
            assert_eq!(
                buf[(LABEL_W, y)].symbol(),
                "\u{2502}",
                "focus bar missing at y={y}"
            );
            assert_eq!(
                buf[(LABEL_W + 1, y)].symbol(),
                " ",
                "padding column non-blank at y={y}"
            );
            assert_eq!(
                buf[(LABEL_W + 2, y)].symbol(),
                "a",
                "value column drift at y={y}"
            );
        }
    }
}
