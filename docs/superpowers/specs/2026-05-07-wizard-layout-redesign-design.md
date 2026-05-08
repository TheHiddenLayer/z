# New Task Wizard Layout Redesign

## Problem

The current `NewAgentPanel` (z's modal for creating an agent session) is
disorienting in practice:

1. **Fields shift position when focus changes.** `prompt_height` flips between
   1 and 3 rows on focus, and `gap_after_*` spacers are derived from leftover
   inner height, so any row growth or shrinkage cascades through the form.
2. **Excessive vertical gaps.** Five independent spacer constraints each
   absorb leftover height, producing large unpredictable blank rows between
   fields.
3. **No visual grouping.** Fields float on the dark background with no
   sectioning, dividers, or focus emphasis stronger than a thin `│` glyph.
4. **Prompt body breaks column alignment.** The prompt text renders across the
   full panel width while every other field obeys the label/value column,
   producing a ragged left edge.

The user goal is a calm, sleek, minimal form whose geometry never reflows and
whose focus is unambiguous.

## Decisions

- Keep the existing label / value column convention. Do not switch to a
  borderless single-column stack or to fully bordered panels.
- Eliminate dynamic spacer math. The form's vertical geometry is fixed; only
  list height adapts to the loaded data.
- The prompt body always reserves its maximum height. It does not grow on
  focus.
- Group dividers are 1-row thin top borders (`─`), not full boxes.
- Focus indicator is a solid left accent bar drawn with
  `Block::new().borders(Borders::LEFT)` on the focused row's value rect.
- Replace the hand-rolled `selectable_source_line` rendering with ratatui's
  built-in `List` widget configured with a stable highlight gutter.
- Stay on ratatui 0.29. Defer the 0.30 bump and a `tui-textarea` swap to
  separate work.

## Layout

### Vertical structure

A single `Layout::vertical` call with `Flex::Start` and `.spacing(0)` produces
the column. Every constraint is `Constraint::Length(fixed)`. No constraint
grows. No spacer math. Group dividers (1-row constraints) carry all the
between-section breathing room; consecutive form rows sit flush.

```
  Repo       solswarm
  Source     [issue]  mr  branch
  Search     filter issues...
             #138 feat: multi-cudagym-backend support
             #137 docs: BYOG
             …                                          (list, up to 6 rows)
  ─────────────────────────────                         (group divider)
  Name       z-0507-138-feat-...
  Prompt     [default]  custom
             generated from issue                       (prompt body row 1)
                                                        (prompt body row 2)
                                                        (prompt body row 3)
  ─────────────────────────────
  Agent      [claude]  codex
  ─────────────────────────────
             enter start · alt+enter newline · esc cancel
```

Row constraints, in order:

| Row              | Height                                     |
| ---------------- | ------------------------------------------ |
| Repo             | `Length(1)`                                |
| Source           | `Length(1)`                                |
| Branch toggle    | `Length(1)` only when source is `branch`   |
| Search           | `Length(1)` only when source is `issue/mr` |
| List             | `Length(LIST_HEIGHT)` (clamped 1..=6)      |
| Group divider    | `Length(1)`                                |
| Name             | `Length(1)` only when source needs a name  |
| Prompt label     | `Length(1)`                                |
| Prompt body      | `Length(PROMPT_BODY_HEIGHT)` always 3      |
| Group divider    | `Length(1)`                                |
| Agent            | `Length(1)`                                |
| Group divider    | `Length(1)`                                |
| Hint bar         | `Length(1)`                                |

Form rows sit flush; the only blank rows in the column are the group divider
rows themselves, which render a single `─`. The form reads as three tight
sections separated by thin horizontal rules.

If the available `area.height` is smaller than the sum of these constraints,
ratatui truncates the bottom rows. We do not attempt to gracefully shrink
inner content; the wizard already requires a minimum height to be useful, and
the existing app prevents opening it in tiny terminals.

### Per-row horizontal split

Every row that has a label is split with:

```rust
let [label_rect, value_rect] = Layout::horizontal([
    Constraint::Length(LABEL_W),
    Constraint::Min(0),
]).areas(row);
```

The label rect renders the label text. The value rect renders the value:
single-line tab strip, single-line text, multi-line `Paragraph`, or a `List`.
Because every row uses the same `LABEL_W`, all values align vertically. The
prompt body renders into its own value rect with `Wrap { trim: false }`, and
all wrapped lines align to the value column without manual `" ".repeat()`
prefixes.

### Group dividers

Each divider is a 1-row rect rendered as
`Block::new().borders(Borders::TOP).border_style(Style::default().fg(DIM))`.
The divider is intentionally dim, so it reads as structure rather than
chrome.

## Focus

Focus is conveyed by two cues:

1. **Left accent bar.** The focused row wraps its value rect in
   `Block::new().borders(Borders::LEFT).border_style(Style::default().fg(TEXT))`
   and renders content inside the block via `Padding::horizontal(1)`. Every
   other row uses `Padding::left(2)` (no block) so widths match. The accent
   bar replaces the current `│ ` indent trick.
2. **Whole-row brightness.** The focused row's label and value paint with
   `TEXT`; unfocused rows paint with `DIM`. This matches the existing agent
   table convention.

Within a row that contains a tab strip (`Source`, `Prompt`, `Agent`), the
selected option remains `Modifier::BOLD`; unselected options remain `DIM`.
Brackets are not added; bold + brightness already encode selection.

## List rendering

Replace `selectable_source_line` and `render_selectable_list` with the
standard `List` widget:

```rust
let items: Vec<ListItem> = entries.iter().map(|e| ListItem::new(label(e))).collect();
let list = List::new(items)
    .highlight_style(Style::default().fg(TEXT).add_modifier(Modifier::BOLD))
    .highlight_symbol("▌ ")
    .highlight_spacing(HighlightSpacing::Always);
let mut state = ListState::default().with_selected(selected_pos);
StatefulWidget::render(list, value_rect, buf, &mut state);
```

`HighlightSpacing::Always` reserves the gutter when nothing is selected, so
list items do not jump horizontally on selection change. The `▌` symbol
matches the row-level focus accent bar.

The scrollbar logic in `render_selectable_list` (current implementation)
stays. It already uses ratatui's stateful `Scrollbar`. We feed the same
`state.offset()` we already compute.

## Prompt body

The prompt body always reserves 3 rows of height. When the prompt is not
focused, it renders the existing summary (e.g. `generated from issue`,
`custom prompt`, the first non-empty line) into row 1; rows 2-3 stay blank.

When the prompt is focused, it renders the full prompt with
`Paragraph::new(text).wrap(Wrap { trim: false }).scroll((scroll, 0))` into
the value rect. The cursor is drawn as a trailing `_` exactly as today.

The dynamic 1↔3 height flip is removed. Removing the flip cascades through
`new_agent_layout_sizing`, which can be deleted along with the optional
spacer machinery.

## What we delete

- `new_agent_layout_sizing` and `NewAgentLayoutSizing` (struct, `cfg(test)`
  helpers, layout-sizing tests).
- `gap_after_source`, `gap_after_agent`, `gap_after_repo`, `gap_after_list`,
  `top_padding` plumbing.
- `selectable_source_line` and `render_selectable_list` (replaced by stock
  `List`).
- The `prompt_height = if is_prompt { 3 } else { 1 }` flip.
- The `" ".repeat(label_w)` indenting hacks for the prompt body, search row,
  and remote status lines: every row now sits inside its own value rect.

## What stays

- The `Mode::NewAgent { .. }` state machine. No state changes.
- The focus rotation order in `app.rs`. Layout changes do not change focus.
- `prompt_summary`, `truncate_end`, `truncate_middle`, `take_prefix_width`,
  `take_suffix_width` helpers. Still useful for fitting text into a value
  rect.
- `filtered_issue_lines`, `filtered_mr_lines`, etc., adapted to return
  `Vec<ListItem>` rather than `Vec<Line>`.
- Hint-bar content per focus state.

## Out of scope

- Switching to `tui-textarea` for the prompt body. Worth doing, separate
  work.
- Upgrading ratatui to 0.30 for `Rect::centered_*`, `MergeStrategy`, and
  dashed border types.
- Turning the wizard into a true centered modal overlay with `Clear`. The
  wizard already lives in the panel slot; relocating it is a separate
  product decision.
- Changing the focus rotation order, the source set, or any keybinding.
- Validation messaging, error rows, or any new field.
- Adding color or accent palette beyond `TEXT` and `DIM`.

## Anti-acceptance criteria

A redesigned wizard fails review if any of the following hold:

- Any row's vertical position changes between two valid focus states.
- The prompt body's wrapped lines render at a different left column than
  other field values.
- The form has more than three group dividers, or any full-box border.
- Focus on a row is conveyed by anything other than the left accent bar
  plus brightness contrast (no color tinting, no underline, no inverted
  background, no box).
- A list selection change shifts other items horizontally.
- `new_agent_layout_sizing` survives in any form.

## Acceptance criteria

- Cycling focus through every field of the wizard produces no row movement
  in any cell of the buffer except the focused row's accent bar and value.
- Selecting a longer prompt or wrapping into 3 lines does not move any row
  below it.
- The Source list, Prompt body, and Search row all render at the same left
  column as Repo, Source, and Agent values.
- The render path no longer references `gap_after_source`,
  `gap_after_agent`, `gap_after_repo`, `gap_after_list`, `top_padding`, or
  `optional_spacer_height`.
- Existing wizard behavior tests continue to pass; new tests cover the
  fixed-geometry guarantee (snapshot of the buffer for two focus states is
  identical except in the focused row).

## Implementation order

1. Replace the vertical layout with `Flex::Start` + `.spacing(1)` and
   fixed-length constraints. Delete the sizing module. Verify the existing
   tests still pass after adjusting expectations.
2. Add the per-row horizontal split helper that returns
   `(label_rect, value_rect)`.
3. Move every field's render to use the helper. Remove
   `" ".repeat(label_w)` from every call site.
4. Swap `render_selectable_list` for stock `List` + `ListState`.
5. Add the focused-row left accent bar.
6. Add the three group dividers.
7. Lock prompt body to 3 rows; rewrite the focused/unfocused branches to
   render into the value rect.
8. Add the focus-stability snapshot test.
