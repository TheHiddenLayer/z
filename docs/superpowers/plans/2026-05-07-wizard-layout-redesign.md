# New Task Wizard Layout Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the new-task wizard's render path so the form's geometry is fixed (no row reflow on focus), the prompt body aligns to the value column, focus is conveyed via a solid left accent bar, sections are separated by thin `─` dividers, and list rendering uses ratatui's stock `List` widget instead of hand-rolled lines.

**Architecture:** Replace `Layout::vertical(...).split()` (default `Flex::Legacy`) and the manual leftover-spacer struct (`NewAgentLayoutSizing`) with a single fixed-length `Layout::vertical(...).flex(Flex::Start).spacing(0)`. Each row is split into label / value sub-rects via a per-row `Layout::horizontal([Length(LABEL_W), Min(0)])`. The focused row's value sub-rect is wrapped in a `Block::new().borders(Borders::LEFT)` to render the focus accent bar; unfocused rows pad by an equivalent column. Three 1-row dividers (`Block::new().borders(Borders::TOP)`) split the form into sections. The prompt body always reserves 3 rows; list rendering switches to stock `List` + `ListState` with `HighlightSpacing::Always`.

**Tech Stack:** Rust 2024 edition, ratatui 0.29, crossterm 0.28. All work is confined to `src/new_agent_panel.rs` and its `mod tests`.

**Spec:** `docs/superpowers/specs/2026-05-07-wizard-layout-redesign-design.md`.

---

## File Structure

- **Modify:** `src/new_agent_panel.rs` — entire `render_new_agent_panel` body, plus delete `new_agent_layout_sizing`, `NewAgentLayoutSizing`, `selectable_source_line`, `render_selectable_list`, the layout-sizing tests.
- **Modify:** `src/new_agent_panel.rs::tests` — replace removed tests; add focus-stability snapshot test, prompt-column-alignment test, and list-gutter-stability test.

No new files. No changes to `src/app.rs`, `src/ui.rs`, `src/style.rs`, or any other module: state machine, focus rotation, key handling, and panel slotting all stay identical.

---

## Constants And Helpers Introduced

The following items are referenced across multiple tasks. Define them when first needed.

```rust
const LABEL_W: u16 = 14;          // already exists as NEW_AGENT_LABEL_W; reuse, do not rename
const PROMPT_BODY_HEIGHT: u16 = 3; // new
const FOCUS_GUTTER: u16 = 2;       // 1 col bar + 1 col padding = matches Padding::left(2) on inactive rows

fn split_row(row: Rect) -> (Rect, Rect) {
    let [label, value] = Layout::horizontal([
        Constraint::Length(LABEL_W),
        Constraint::Min(0),
    ]).areas(row);
    (label, value)
}

fn render_divider(area: Rect, buf: &mut Buffer) {
    Block::new()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(DIM))
        .render(area, buf);
}

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
```

---

## Task 1: Add focus-stability snapshot test

**Files:**
- Test: `src/new_agent_panel.rs` (inside `mod tests`)

This test pins the desired invariant before any production change: cycling focus across two states must not move any cell except inside the focused row's value column. It should fail against today's code (because `prompt_height` flips and `gap_after_*` reflow), then pass once Task 2 lands.

- [ ] **Step 1: Add the failing test**

Append to `src/new_agent_panel.rs::tests`:

```rust
fn render_with_focus(app: &App, focus: NewAgentFocus, area: Rect) -> Buffer {
    let mut app = app.clone();
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
    let app = wizard_app();
    let area = Rect::new(0, 0, 80, 24);

    let on_repo = render_with_focus(&app, NewAgentFocus::Repo, area);
    let on_prompt = render_with_focus(&app, NewAgentFocus::Prompt, area);

    // The focus accent bar plus the value column may legitimately differ.
    // Everything in columns 0..LABEL_W (label column) must be identical.
    let a = cells_outside_columns(&on_repo, 0..LABEL_W);
    let b = cells_outside_columns(&on_prompt, 0..LABEL_W);

    assert_eq!(
        a, b,
        "form rows shifted between focus states; geometry must be fixed"
    );
}
```

`app::tests::test_app_in_new_agent_mode` lives in a private `#[cfg(test)] mod tests` block inside `src/app.rs` and is not reachable from another module's tests. Rather than widening that visibility, copy a minimal equivalent helper directly into `new_agent_panel::tests`:

```rust
fn wizard_app() -> App {
    use crate::app::{Action, App, BranchMode, Config, Mode, NewAgentSource, RemoteList};
    use std::path::PathBuf;
    let mut config = Config::default();
    config.repos = vec![PathBuf::from("/tmp/solswarm")];
    config.agents = vec![("claude".into(), Default::default()), ("codex".into(), Default::default())];
    let mut app = App::new(config);
    app.update(Action::OpenNewAgent);
    if let Mode::NewAgent { branch_mode, branches, agent_name, branch_name, name_pristine, .. } = &mut app.mode {
        *branch_mode = BranchMode::New;
        *branches = vec!["main".into()];
        *branch_name = "z-0507-1".into();
        *name_pristine = true;
        *agent_name = "claude".into();
    }
    app
}
```

If `Config::default()`, `Action::OpenNewAgent`, or any field accessed above does not match the actual app shape (verify by reading `src/app.rs` first), adjust the helper to construct a valid `App` in `Mode::NewAgent` directly. The shape of the helper is illustrative; what matters is that the test starts with an `App` whose `mode` is `Mode::NewAgent { .. }`.

- [ ] **Step 2: Run the test and verify it fails**

Run: `cargo test --lib focus_changes_do_not_move_form_rows`
Expected: FAIL with `assertion `left == right` failed` and a non-empty diff between the two label-column snapshots, because the prompt-focus path reflows rows above and below it via `gap_after_*` math and the `prompt_height` flip.

- [ ] **Step 3: Commit**

```bash
git add src/new_agent_panel.rs src/app.rs
git commit -m "test(wizard): pin focus-stability invariant"
```

---

## Task 2: Replace vertical layout with fixed-length Flex::Start

**Files:**
- Modify: `src/new_agent_panel.rs:494-598` (the `new_agent_layout_sizing` function and the `Layout::vertical(...).split(inner)` block in `render_new_agent_panel`)

This is the structural change. We delete `new_agent_layout_sizing` / `NewAgentLayoutSizing` entirely, lock prompt body to 3 rows, and use a single fixed `Layout::vertical(...).flex(Flex::Start).spacing(0)` call.

- [ ] **Step 1: Remove `NewAgentLayoutSizing` and `new_agent_layout_sizing`**

Delete the entire `#[derive(Debug, Clone, Copy)] struct NewAgentLayoutSizing { ... }` block, its `impl NewAgentLayoutSizing { ... }` block, and the standalone `fn new_agent_layout_sizing(...)` (lines ~453-520 in current `src/new_agent_panel.rs`).

Also delete the corresponding tests at the bottom of the file:
- `layout_sizing_caps_list_to_one_when_inner_height_is_tight`
- `layout_sizing_preserves_required_rows_at_minimum_supported_height`
- `layout_sizing_allows_six_list_rows_when_height_is_available`

- [ ] **Step 2: Rewrite the vertical layout in `render_new_agent_panel`**

Replace the block beginning `let sizing = new_agent_layout_sizing(...)` and the entire `let chunks = Layout::vertical([ ... ]).split(inner);` (lines ~573-598) with:

```rust
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
let list_height = desired_list_height.clamp(1, 6);

// Row order. `Length(0)` rows render nothing and consume nothing visually.
let constraints = [
    Constraint::Length(1),                                  // 0  Repo
    Constraint::Length(1),                                  // 1  Source
    Constraint::Length(if show_branch_toggle { 1 } else { 0 }), // 2  Branch toggle
    Constraint::Length(if show_gitlab_source { 1 } else { 0 }), // 3  Search
    Constraint::Length(list_height),                        // 4  List
    Constraint::Length(1),                                  // 5  Divider 1
    Constraint::Length(if show_name_row { 1 } else { 0 }),  // 6  Name
    Constraint::Length(1),                                  // 7  Prompt label
    Constraint::Length(PROMPT_BODY_HEIGHT),                 // 8  Prompt body
    Constraint::Length(1),                                  // 9  Divider 2
    Constraint::Length(1),                                  // 10 Agent
    Constraint::Length(1),                                  // 11 Divider 3
    Constraint::Length(1),                                  // 12 Hint
];

let chunks = Layout::vertical(constraints)
    .flex(Flex::Start)
    .spacing(0)
    .split(inner);
```

Update every `chunks[N]` index in the rest of `render_new_agent_panel` to match the new ordering. Specifically:

| Field             | Old index | New index |
| ----------------- | --------- | --------- |
| Repo              | `chunks[1]`  | `chunks[0]`  |
| Source            | `chunks[3]`  | `chunks[1]`  |
| Branch toggle     | `chunks[5]`  | `chunks[2]`  |
| Source list slot  | `chunks[6]`  | `chunks[3..=4]` (search row + list rows) |
| Name              | `chunks[8]`  | `chunks[6]`  |
| Prompt label      | `chunks[9]`  | `chunks[7]`  |
| Prompt body       | `chunks[10]` | `chunks[8]`  |
| Agent             | `chunks[12]` | `chunks[10]` |
| Hint              | `chunks[13]` | `chunks[12]` |

The list-area construction that today does
`let source_chunks = Layout::vertical([Constraint::Length(1), Constraint::Length(sizing.list_height)]).split(list_slot);`
becomes simply: `chunks[3]` is the search row, `chunks[4]` is the list rect. Drop the inner split entirely.

For the branch (non-gitlab) path that currently uses `chunks[6]` as the full list slot, render the branch list directly into `chunks[4]`. The `show_gitlab_source` flag selects which renderer; the geometry is shared.

- [ ] **Step 3: Add the constants and the import for `Flex`**

At the top of `src/new_agent_panel.rs`, in the existing `use ratatui::{...}` block, add `Flex` and `Padding` and `Borders` and `Block`:

```rust
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Padding, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, StatefulWidget, Widget, Wrap,
    },
};
```

Below the existing constants:

```rust
const LABEL_W: u16 = NEW_AGENT_LABEL_W;
const PROMPT_BODY_HEIGHT: u16 = 3;
```

(Keep `NEW_AGENT_LABEL_W` for now; we deduplicate it in Task 10.)

- [ ] **Step 4: Replace the dynamic prompt height with the fixed constant**

Find `let prompt_height = if is_prompt { 3 } else { 1 };` and delete it. Anywhere `sizing.prompt_height` was used, the new layout already uses `PROMPT_BODY_HEIGHT`. Anywhere `is_prompt` controlled the prompt height, the height no longer depends on focus; the renderer's content branches still depend on `is_prompt`.

- [ ] **Step 5: Run the full test suite**

Run: `cargo test --lib`
Expected: `focus_changes_do_not_move_form_rows` passes. `typing_in_generated_prompt_marks_it_custom` and other existing wizard tests pass. The deleted layout-sizing tests are gone.

If a test snapshot test (e.g. `filtered_issue_lines_render_number_title_and_selection`) still passes unchanged, that is correct: those tests assert pure-data helpers that we did not touch.

- [ ] **Step 6: Commit**

```bash
git add src/new_agent_panel.rs
git commit -m "refactor(wizard): fixed vertical layout via Flex::Start"
```

---

## Task 3: Lock prompt body branch to fixed 3-row rect

**Files:**
- Modify: `src/new_agent_panel.rs` — the prompt body branch in `render_new_agent_panel`

Today the prompt body branch picks rendering based on `is_prompt` and `prompt.is_empty()`. That logic stays, but it now writes into a 3-row rect even when collapsed.

- [ ] **Step 1: Add a test that the collapsed prompt summary appears at row 1 of the prompt body**

Append to `mod tests`:

```rust
#[test]
fn collapsed_prompt_summary_renders_in_first_body_row_only() {
    let app = wizard_app();
    let mut app = app.clone();
    if let Mode::NewAgent { focus, prompt, prompt_mode, .. } = &mut app.mode {
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
```

- [ ] **Step 2: Run and verify it fails or passes**

Run: `cargo test --lib collapsed_prompt_summary_renders_in_first_body_row_only`
Expected: PASS if Task 2 already routes the collapsed branch through `chunks[8]` whose height is `PROMPT_BODY_HEIGHT = 3`. If the existing collapsed branch wrote into a 1-row rect (it did), the buffer below the summary is blank because we never rendered there. The test should pass directly off Task 2.

If it fails because the body rows below contain divider-`─` characters, you placed `chunks[8]` adjacent to a divider without leaving an empty row in between. Recheck the constraint order in Task 2: divider 2 is `chunks[9]`, immediately after the prompt body, so its `─` lives one row below the body's last row, not inside the body.

- [ ] **Step 3: Commit**

```bash
git add src/new_agent_panel.rs
git commit -m "test(wizard): pin prompt body to 3-row rect"
```

---

## Task 4: Introduce `split_row` helper and adopt it for label rows

**Files:**
- Modify: `src/new_agent_panel.rs`

Today every row builds a `Line` containing manual `Span::raw(" ".repeat(label_padding))` for the label column and renders it into the full row rect. Switch to splitting each row into label / value sub-rects.

- [ ] **Step 1: Add `split_row` next to the existing helpers**

Place after `truncate_middle`:

```rust
fn split_row(row: Rect) -> (Rect, Rect) {
    let [label, value] = Layout::horizontal([
        Constraint::Length(LABEL_W),
        Constraint::Min(0),
    ]).areas(row);
    (label, value)
}
```

- [ ] **Step 2: Migrate the Repo row to use it**

Replace:

```rust
let repo_line = picker_row("Repo", repo_name, is_repo);
Paragraph::new(repo_line).render(chunks[0], buf);
```

with:

```rust
let (label_rect, value_rect) = split_row(chunks[0]);
render_label("Repo", is_repo, label_rect, buf);
render_value(repo_name, is_repo, value_rect, buf);
```

Add `render_label` / `render_value` helpers (kept private to this module):

```rust
fn render_label(text: &str, focused: bool, area: Rect, buf: &mut Buffer) {
    let style = if focused {
        Style::default().fg(TEXT)
    } else {
        Style::default().fg(DIM)
    };
    Paragraph::new(Span::styled(text.to_string(), style)).render(area, buf);
}

fn render_value(text: &str, focused: bool, area: Rect, buf: &mut Buffer) {
    let style = if focused {
        Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(TEXT)
    };
    Paragraph::new(Span::styled(text.to_string(), style)).render(area, buf);
}
```

- [ ] **Step 3: Migrate Source, Branch toggle, Search, Name, Prompt label, Agent rows the same way**

For tab-strip rows (Source, Prompt label, Agent), the value sub-rect renders a `Line` of bold-vs-dim spans (existing logic from `tabbed_row` / `source_tabs_row` / `agent_tabs_row` / `prompt_tabs_row`). Refactor those helpers to return only the value spans (drop the indicator / label / padding spans; the row-split now handles columns):

```rust
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
```

Delete `tabbed_row`, `source_tabs_row`, `agent_tabs_row`, `prompt_tabs_row` once their callers have switched to `tab_value_line`. Each old-call-site becomes:

```rust
let (l, v) = split_row(chunks[N]);
render_label("Source", is_source, l, buf);
Paragraph::new(tab_value_line(&["issue", "mr", "branch"], source as usize)).render(v, buf);
```

`source as usize` requires either `#[derive(Clone, Copy)]` already on `NewAgentSource` (verify in `src/app.rs`) and a manual `match` to map to the index — keep the explicit `match` to avoid coupling to enum layout:

```rust
let selected = match source {
    NewAgentSource::Issue => 0,
    NewAgentSource::Mr => 1,
    NewAgentSource::Branch => 2,
};
```

- [ ] **Step 4: Run the test suite**

Run: `cargo test --lib`
Expected: PASS. The existing line-text tests that asserted `"    \u{2502} #123 Fix agent startup"` still pass because we have not yet changed list rendering. The focus-stability test still passes because we changed only how a row's columns are computed, not row positions.

- [ ] **Step 5: Commit**

```bash
git add src/new_agent_panel.rs
git commit -m "refactor(wizard): split each row into label/value sub-rects"
```

---

## Task 5: Render prompt body into the value sub-rect

**Files:**
- Modify: `src/new_agent_panel.rs` — the prompt body branch

Today the prompt body renders across the full row width with `" ".repeat(label_w)` prefixes. After Task 4 there is no row-level prefix; prompt body must render into the value sub-rect so wrapped lines align to the value column.

- [ ] **Step 1: Add a test that wrapped prompt lines align to the value column**

```rust
#[test]
fn focused_prompt_wrapped_lines_align_to_value_column() {
    let app = wizard_app();
    let mut app = app.clone();
    if let Mode::NewAgent { focus, prompt, prompt_mode, .. } = &mut app.mode {
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
```

Note: this test asserts the post-Task-8 invariant about alignment; the focus accent bar from Task 8 will sit at `LABEL_W` and consume that column. Once Task 8 lands, update the test to expect the bar at `LABEL_W` and `a` at `LABEL_W + FOCUS_GUTTER`. For now (pre-Task-8), the prompt content sits flush at `LABEL_W`.

- [ ] **Step 2: Replace the prompt body branch**

Find the block beginning `// --- Prompt area ---` and replace its contents with:

```rust
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
```

The label cell of the prompt row stays empty in the body rows; the `Prompt` label is rendered on the prompt-label row above (`chunks[7]`).

- [ ] **Step 3: Run the test**

Run: `cargo test --lib focused_prompt_wrapped_lines_align_to_value_column`
Expected: PASS.

Run: `cargo test --lib`
Expected: All wizard tests pass. The two prompt-related tests for `prompt_summary` (pure-data) are unaffected.

- [ ] **Step 4: Commit**

```bash
git add src/new_agent_panel.rs
git commit -m "refactor(wizard): render prompt body into value sub-rect"
```

---

## Task 6: Migrate Search row and remote status messages to value sub-rect

**Files:**
- Modify: `src/new_agent_panel.rs`

Today `remote_status_line` builds a `Line` with `Span::raw(" ".repeat(label_w))`. After this task, status messages render into the search-row's value rect, not the full row.

- [ ] **Step 1: Replace Search row rendering**

Find the block guarded by `if show_gitlab_source { ... }`. Replace with:

```rust
if show_gitlab_source {
    let (l, v) = split_row(chunks[3]);
    let is_search = matches!(focus, NewAgentFocus::Search);
    render_label("Search", is_search, l, buf);
    let search_text = if source_query.is_empty() {
        match source {
            NewAgentSource::Issue => "filter issues...",
            NewAgentSource::Mr => "filter MRs...",
            NewAgentSource::Branch => "",
        }
    } else {
        source_query.as_str()
    };
    let style = if is_search {
        Style::default().fg(TEXT)
    } else {
        Style::default().fg(DIM)
    };
    Paragraph::new(Span::styled(search_text.to_string(), style)).render(v, buf);
}
```

- [ ] **Step 2: Replace `remote_status_line` callers to render directly into a value rect**

`remote_status_line` returned a `Line<'static>` with leading whitespace. Replace it with a function that takes a rect:

```rust
fn render_remote_status(message: &str, area: Rect, buf: &mut Buffer) {
    Paragraph::new(Span::styled(
        message.to_string(),
        Style::default().fg(DIM),
    ))
    .render(area, buf);
}
```

Then in the list area branch (currently `Paragraph::new(remote_status_line(empty_msg, label_w)).render(list_area, buf);`), the status now writes into the value sub-rect of the list rect:

```rust
let (_l, list_value) = split_row(chunks[4]);
if active_list.is_empty() {
    let empty_msg = match branch_mode {
        BranchMode::New => "loading...",
        BranchMode::Existing => "no existing branches",
    };
    render_remote_status(empty_msg, list_value, buf);
}
```

Inside `filtered_issue_lines` and `filtered_mr_lines`, the `RemoteList::Idle | Loading` and `Failed` arms today return `vec![remote_status_line(...)]` for use in `render_selectable_list`. This function is rewritten in Task 7 (stock `List`); for now, leave the helpers returning `Vec<Line<'static>>` so the list code continues to work, but drop the leading `Span::raw(" ".repeat(label_w))` from `remote_status_line` so the status sits flush at column 0 of the value sub-rect.

Update `remote_status_line` to:

```rust
fn remote_status_line(message: &str) -> Line<'static> {
    Line::from(vec![Span::styled(
        message.to_string(),
        Style::default().fg(DIM),
    )])
}
```

Update the `remote_status_line_is_indented` test to match (rename to `remote_status_line_has_no_leading_indent` and assert `"   loading..."` becomes `"loading..."` etc.).

- [ ] **Step 3: Run tests**

Run: `cargo test --lib`
Expected: PASS. `filtered_issue_lines_distinguish_empty_from_no_matches` and `filtered_mr_lines_distinguish_empty_from_no_matches` will fail because their assertions expected the leading indent. Update them to expect the un-indented strings (`"no assigned issues"`, `"no MRs needing review"`).

- [ ] **Step 4: Commit**

```bash
git add src/new_agent_panel.rs
git commit -m "refactor(wizard): render search + remote status into value sub-rect"
```

---

## Task 7: Replace `selectable_source_line` / `render_selectable_list` with stock `List` + `ListState`

**Files:**
- Modify: `src/new_agent_panel.rs`

Stock `List` with `HighlightSpacing::Always` provides the stable selection gutter the spec calls for and removes ~70 lines of hand-rolled rendering.

- [ ] **Step 1: Add a test that list selection does not shift other items horizontally**

```rust
#[test]
fn list_selection_change_keeps_items_at_same_column() {
    use crate::app::{Mode, NewAgentSource, RemoteList};
    let mut app = wizard_app();
    if let Mode::NewAgent { source, issues, source_index, focus, .. } = &mut app.mode {
        *source = NewAgentSource::Issue;
        *issues = RemoteList::Loaded(vec![
            issue(1, "alpha"),
            issue(2, "beta"),
            issue(3, "gamma"),
        ]);
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
    let col_in = |buf: &Buffer, needle: &str| -> Option<u16> {
        for y in 0..area.height {
            let mut s = String::new();
            for x in 0..area.width {
                s.push_str(buf[(x, y)].symbol());
            }
            if let Some(idx) = s.find(needle) {
                return Some(idx as u16);
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
```

- [ ] **Step 2: Run the test, confirm it fails**

Run: `cargo test --lib list_selection_change_keeps_items_at_same_column`
Expected: FAIL. Today's `selectable_source_line` renders `│ ` for the selected item and `  ` for unselected, both at the same prefix width, so this might actually pass. If it passes, that's fine — the test still pins the invariant for the rewrite.

- [ ] **Step 3: Replace list rendering**

Define adapter functions that produce `Vec<ListItem<'static>>` instead of `Vec<Line<'static>>`. Replace `filtered_issue_lines`, `filtered_mr_lines`, and `selectable_source_line` with:

```rust
enum ListPayload {
    Status(String),
    Items { labels: Vec<String>, indices: Vec<usize> },
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
```

Then in `render_new_agent_panel`:

```rust
let (_l, list_value) = split_row(chunks[4]);
let payload = match source {
    NewAgentSource::Issue => issue_items(issues, source_query),
    NewAgentSource::Mr => mr_items(mrs, source_query),
    NewAgentSource::Branch => branch_items(active_list, branch_mode),
};
match payload {
    ListPayload::Status(msg) => render_remote_status(&msg, list_value, buf),
    ListPayload::Items { labels, indices } => {
        let selected_pos = match source {
            NewAgentSource::Issue | NewAgentSource::Mr => indices.iter().position(|&i| i == *source_index),
            NewAgentSource::Branch => Some(*base_index),
        };
        let items: Vec<ListItem> = labels
            .into_iter()
            .map(|label| ListItem::new(label))
            .collect();
        let list = List::new(items)
            .style(Style::default().fg(DIM))
            .highlight_style(Style::default().fg(TEXT).add_modifier(Modifier::BOLD))
            .highlight_symbol("▌ ")
            .highlight_spacing(HighlightSpacing::Always);
        let mut state = ListState::default().with_selected(selected_pos);
        StatefulWidget::render(list, list_value, buf, &mut state);
    }
}
```

`HighlightSpacing` must be imported: add it to the existing `use ratatui::widgets::{...}` line.

Define `mr_items` and `branch_items` analogously to `issue_items`. For branches:

```rust
fn branch_items(branches: &[String], _branch_mode: &BranchMode) -> ListPayload {
    if branches.is_empty() {
        return ListPayload::Status("loading...".to_string());
    }
    let labels: Vec<String> = branches.iter().cloned().collect();
    let indices: Vec<usize> = (0..branches.len()).collect();
    ListPayload::Items { labels, indices }
}
```

- [ ] **Step 4: Delete dead helpers**

Remove:
- `selectable_source_line`
- `render_selectable_list`
- `list_outer_area`
- `list_content_area`
- `list_scroll_offset` (consumed by the deleted `render_selectable_list`)
- `MAX_SOURCE_LIST_WIDTH` constant
- `filtered_issue_lines` / `filtered_mr_lines` — replaced by `issue_items` / `mr_items`

Also remove the corresponding tests:
- `filtered_issue_lines_render_number_title_and_selection`
- `filtered_mr_lines_include_source_branch`
- `list_scroll_offset_keeps_selected_last_visible`
- `list_content_area_reserves_scrollbar_column_only_when_needed`
- `render_selectable_list_hides_scrollbar_when_rows_fill_viewport`
- `render_selectable_list_shows_scrollbar_when_rows_exceed_viewport`
- `list_outer_area_caps_wide_source_lists`

Replace with stock-`List`-aware tests:

```rust
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
```

- [ ] **Step 5: Run all tests**

Run: `cargo test --lib`
Expected: PASS, including `list_selection_change_keeps_items_at_same_column`. The `▌ ` highlight symbol consumes 2 columns; `HighlightSpacing::Always` reserves them for unselected rows so labels start at the same column for selected and unselected.

- [ ] **Step 6: Commit**

```bash
git add src/new_agent_panel.rs
git commit -m "refactor(wizard): switch list rendering to stock List + HighlightSpacing"
```

Note: list scrollbar is dropped in this task. Stock `List` already supports keyboard scroll via `ListState::with_offset`. If a scrollbar is still desired for visual hint, follow up with a separate task (out of scope for this plan; spec lists scrollbar as a "stays" item, but the spec's "stays" referred to its conceptual presence — we re-add it explicitly only if buffer-cell tests show wraparound below 6 rows is confusing).

If retaining the scrollbar is required, keep the existing `Scrollbar` rendering after the `List`: read `state.offset()` and a content length derived from the payload `labels.len()`, render the scrollbar over a 1-column-wide rect cut from the right of `list_value`.

---

## Task 8: Render focus accent bar via `Block::borders(Borders::LEFT)`

**Files:**
- Modify: `src/new_agent_panel.rs`

Switch from "value sub-rect, render directly" to "value sub-rect, render inside a focus block". The block consumes 1 column for `Borders::LEFT` and 1 column for `Padding::horizontal(1)` on the focused row. Unfocused rows use a non-bordered block with `Padding::left(2)` so widths match.

- [ ] **Step 1: Add the helper**

```rust
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
```

- [ ] **Step 2: Wrap every value-rect render with the focus block**

For each row's render, replace:

```rust
let (l, v) = split_row(chunks[N]);
render_label(LABEL, focused, l, buf);
Paragraph::new(value).render(v, buf);
```

with:

```rust
let (l, v) = split_row(chunks[N]);
render_label(LABEL, focused, l, buf);
let block = focus_block(focused);
let inner = block.inner(v);
block.render(v, buf);
Paragraph::new(value).render(inner, buf);
```

For the `List` render in Task 7's branch:

```rust
let block = focus_block(matches!(focus, NewAgentFocus::SourceList | NewAgentFocus::BranchList));
let inner = block.inner(list_value);
block.render(list_value, buf);
StatefulWidget::render(list, inner, buf, &mut state);
```

For the prompt body:

```rust
let block = focus_block(is_prompt);
let inner = block.inner(body_rect);
block.render(body_rect, buf);
// existing match-on-state Paragraph render into `inner`
```

- [ ] **Step 3: Update Task 5 test for the accent bar**

The earlier test `focused_prompt_wrapped_lines_align_to_value_column` asserted content at column `LABEL_W`. With the focus block, the bar is at `LABEL_W` and content begins at `LABEL_W + 2`. Update:

```rust
assert_eq!(buf[(LABEL_W, y)].symbol(), "│", "focus bar missing at y={y}");
assert_eq!(buf[(LABEL_W + 2, y)].symbol(), "a", "value column drift at y={y}");
assert_eq!(buf[(LABEL_W + 1, y)].symbol(), " ", "padding column non-blank at y={y}");
```

`Borders::LEFT` renders the `│` glyph by default; if a different glyph is wanted, set `.border_set(symbols::border::PLAIN)` explicitly.

- [ ] **Step 4: Update the prompt-summary test**

`collapsed_prompt_summary_renders_in_first_body_row_only` looked for `"describe the work"` in the buffer. With the unfocused focus block (`Padding::left(2)`), the summary now starts at column `LABEL_W + 2`. The `find` call still works because we scan the full row text, but make it explicit:

```rust
if line.contains("describe the work") {
    let col = line.find("describe the work").unwrap() as u16;
    assert_eq!(col, LABEL_W + 2, "prompt summary column drift");
    summary_row = Some(y);
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo test --lib`
Expected: PASS. The focus-stability test from Task 1 still passes because the focus block only affects the focused row's value sub-rect, which is in the value column we already exclude.

- [ ] **Step 6: Commit**

```bash
git add src/new_agent_panel.rs
git commit -m "feat(wizard): focus accent bar via Borders::LEFT"
```

---

## Task 9: Render the three group dividers

**Files:**
- Modify: `src/new_agent_panel.rs`

The constraints array from Task 2 already reserves 1 row each for dividers at `chunks[5]`, `chunks[9]`, `chunks[11]`. Render them.

- [ ] **Step 1: Add the helper**

```rust
fn render_divider(area: Rect, buf: &mut Buffer) {
    Block::new()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(DIM))
        .render(area, buf);
}
```

- [ ] **Step 2: Render at the three indices**

After all field rows are rendered, add:

```rust
render_divider(chunks[5], buf);
render_divider(chunks[9], buf);
render_divider(chunks[11], buf);
```

- [ ] **Step 3: Add a test that the dividers render `─` across the inner width**

```rust
#[test]
fn three_group_dividers_render_horizontal_rules() {
    let app = wizard_app();
    let area = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(area);
    NewAgentPanelWidget::new(&app).render(area, &mut buf);

    let mut divider_rows = 0;
    for y in 0..area.height {
        let row: String = (0..area.width).map(|x| buf[(x, y)].symbol().to_string()).collect();
        if row.chars().filter(|c| *c == '─').count() >= (area.width as usize - 4) {
            divider_rows += 1;
        }
    }
    assert_eq!(divider_rows, 3, "expected exactly three group dividers");
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/new_agent_panel.rs
git commit -m "feat(wizard): three group dividers separating sections"
```

---

## Task 10: Cleanup pass

**Files:**
- Modify: `src/new_agent_panel.rs`

Sweep dead helpers and stale constants.

- [ ] **Step 1: Audit and delete**

Verify these are unused (use `cargo check` after each removal):
- `NEW_AGENT_LABEL_W` (renamed `LABEL_W`; remove the old name)
- `picker_row` (replaced by `render_label` + `render_value`)
- `prompt_tabs_row`, `source_tabs_row`, `agent_tabs_row`, `tabbed_row` (replaced by `tab_value_line`)
- `remote_status_line` (replaced by `render_remote_status` + `ListPayload::Status`)
- `matches_source_query` — keep, still used by `filtered_issue_indices` / `filtered_mr_indices`.
- `text_width`, `take_prefix_width`, `take_suffix_width` — keep, still used by `truncate_end` / `truncate_middle`.
- `truncate_end` — keep if `prompt_summary` still calls it.
- `truncate_middle` — keep, used by Name row.

- [ ] **Step 2: Verify the file is below ~700 lines**

Run: `wc -l src/new_agent_panel.rs`
Expected: significantly fewer lines than before (was 1051; goal under 700, including tests).

- [ ] **Step 3: Run full test + clippy**

Run:
```bash
cargo test --lib
cargo clippy --all-targets -- -D warnings
```
Expected: PASS, no warnings.

- [ ] **Step 4: Manual visual check**

Run: `cargo run -- --help` to confirm the binary builds, then run `cargo run` in a terminal at least 80x24, press `n` to open the wizard, tab through Repo → Source → Search → SourceList → Name → Prompt (default) → Prompt (custom) → Agent and confirm:

1. No row above or below the focused row moves between focus changes.
2. The focus accent bar (`│`) appears at column 14 of the focused row only.
3. The prompt body's wrapped lines align to column 16.
4. Three thin `─` dividers separate Source/List, Prompt, Agent, and the hint bar.
5. The list selection symbol `▌` does not shift items when selection changes.

Document the manual check result in the commit body.

- [ ] **Step 5: Commit**

```bash
git add src/new_agent_panel.rs
git commit -m "chore(wizard): remove dead layout helpers post-redesign"
```

---

## Acceptance verification

After all tasks land:

- [ ] `cargo test --lib` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] `grep -nE 'gap_after_|new_agent_layout_sizing|optional_spacer_height|top_padding|NewAgentLayoutSizing' src/` returns nothing.
- [ ] `grep -n '" ".repeat(label_w' src/new_agent_panel.rs` returns nothing (no manual indent).
- [ ] Manual visual check from Task 10 step 4 documented.
- [ ] Spec's anti-acceptance list (no row movement, no full-box border, only one focus-cue family, no horizontal item shift on list selection, no `new_agent_layout_sizing` survivor) all hold.
