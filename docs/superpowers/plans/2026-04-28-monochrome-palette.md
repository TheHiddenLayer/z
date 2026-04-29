# Monochrome Palette Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drop `ACCENT` (cyan) from z's TUI palette. Encode every selection / focus signal that previously used cyan via brightness contrast (`TEXT` vs `DIM`) and structural glyphs (`│` left bar, `_` cursor, bold-TEXT for inline noun emphasis).

**Architecture:** No new modules, no new helpers. Change inline `Style::default().fg(ACCENT)` calls to `Style::default().fg(TEXT)` (or remove the focus distinction entirely where the spec calls for it). Migrate one render surface per task so each commit is a small, reviewable visual change. Final task removes the `ACCENT` constant — any leftover reference becomes a compile error, which is the strongest possible regression guard.

**Tech Stack:** Rust 2024, ratatui 0.29. No new deps. Unit tests in `src/style.rs` (11 existing); UI migrations are visual-only and verified by compile + manual `cargo run`.

**Spec reference:** `docs/superpowers/specs/2026-04-28-monochrome-palette-design.md` (commit 528883e).

---

## File Structure

| File | Status | Responsibility |
|------|--------|----------------|
| `src/style.rs` | modify | Remove `ACCENT` constant; update tests; rewrite module doc. |
| `src/ui.rs` | modify | Migrate every `ACCENT` callsite (8 surfaces) to monochrome. |

`src/main.rs`, `src/app.rs`, `src/agent.rs`, `Cargo.toml`: unchanged.

---

## Migration ordering

`ACCENT` stays defined until Task 9. Each migration task changes one render surface inline, leaves the constant in place, runs the existing test suite, and commits. Task 9 removes the constant — any leftover usage becomes a compile error.

---

## Task 1: Migrate agent table selection

**Why first:** Highest-traffic surface; visually validates the brightness-contrast pattern that subsequent tasks will reuse.

**Files:**
- Modify: `src/ui.rs:140-154`

- [ ] **Step 1.1: Replace ACCENT with TEXT on the selected indicator and selected text style**

In `src/ui.rs`, locate the block at lines 140-154 (inside `draw_agent_table`'s `for` loop):

```rust
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
```

Replace with:

```rust
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
```

- [ ] **Step 1.2: Verify it builds and existing tests pass**

Run: `cargo build`
Expected: builds cleanly. (`ACCENT` is still imported and used by other surfaces — no warnings yet.)

Run: `cargo test`
Expected: all existing tests pass. The `tokens_have_expected_colors` test still asserts `ACCENT == Color::Cyan` and passes; we haven't touched the constant.

- [ ] **Step 1.3: Manual verify**

Run: `cargo run`
Expected: agent table's selected row shows the `│` bar at terminal-default brightness (white/light) instead of cyan, with selected branch/repo text also in `TEXT` brightness. Unselected rows stay DIM. Status glyphs (✓/spinner/✗/−) keep their colors. Quit with `q`.

- [ ] **Step 1.4: Commit**

```bash
git add src/ui.rs
git commit -m "Agent table selection: brightness contrast, no cyan"
```

---

## Task 2: Migrate separator branch label

**Files:**
- Modify: `src/ui.rs:210-225` (inside `draw_separator`)

- [ ] **Step 2.1: Replace ACCENT with TEXT on the separator's selected-agent label**

Locate the block at lines 210-225:

```rust
    let label_spans = if let Some(agent) = app.selected_agent() {
        let dim_style = Style::default().fg(DIM);
        let accent_style = Style::default().fg(ACCENT);

        let drifted = agent.slug != agent.branch.replace('/', "-");
        let mut spans = vec![Span::styled(" ", dim_style)];
        if drifted {
            spans.push(Span::styled(agent.slug.as_str(), accent_style));
            spans.push(drift_arrow());
            spans.push(Span::styled(
                agent.branch.as_str(),
                accent_style.add_modifier(Modifier::ITALIC),
            ));
        } else {
            spans.push(Span::styled(agent.branch.as_str(), accent_style));
        }
```

Replace with (rename local `accent_style` → `label_style` to match its new role):

```rust
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
```

- [ ] **Step 2.2: Verify build + tests**

Run: `cargo build && cargo test`
Expected: clean build, all tests pass.

- [ ] **Step 2.3: Manual verify**

Run: `cargo run`
Expected: the separator's branch label (right side, e.g., ` my-branch `) renders in `TEXT` brightness against the surrounding DIM dashes — no cyan. The dot strip on the left keeps per-agent status colors (green/yellow/red).

- [ ] **Step 2.4: Commit**

```bash
git add src/ui.rs
git commit -m "Separator label: TEXT brightness, no cyan"
```

---

## Task 3: Migrate modal field label/value style closures

**Files:**
- Modify: `src/ui.rs:375-381` (inside `draw_new_agent_modal`)

These two closures are used by the Name field row (and were used by other rows before `picker_row` was extracted). Per the spec, label keeps the focused/unfocused brightness toggle (TEXT/DIM); value loses the toggle entirely (always TEXT).

- [ ] **Step 3.1: Replace ACCENT in label_style and val_style**

Locate lines 375-381:

```rust
    let label_w = 14u16;
    let label_style = |focused: bool| {
        if focused { Style::default().fg(ACCENT) } else { Style::default().fg(DIM) }
    };
    let val_style = |focused: bool| {
        if focused { Style::default().fg(ACCENT) } else { Style::default().fg(TEXT) }
    };
```

Replace with:

```rust
    let label_w = 14u16;
    let label_style = |focused: bool| {
        if focused { Style::default().fg(TEXT) } else { Style::default().fg(DIM) }
    };
    let val_style = |_focused: bool| Style::default().fg(TEXT);
```

The `_focused` parameter is preserved (still passed by callers) but its value is now ignored — value text is content, not a focus indicator.

- [ ] **Step 3.2: Verify build + tests**

Run: `cargo build && cargo test`
Expected: clean build (no unused-variable warning because the `_` prefix silences it), all tests pass.

- [ ] **Step 3.3: Commit**

```bash
git add src/ui.rs
git commit -m "New Agent modal field styles: brightness on label, content-only on value"
```

---

## Task 4: Migrate picker_row internals

**Files:**
- Modify: `src/ui.rs:383-412` (inside `draw_new_agent_modal`)

`picker_row` is used for the Agent / Repo / Branch-toggle rows. Its internal styles currently shadow the outer `label_style` / `val_style` closures with their own ACCENT-driven versions.

- [ ] **Step 4.1: Replace ACCENT in picker_row's three style closures**

Locate lines 383-412:

```rust
    // Picker row: "│ Label    value" when focused, "  Label    value" when not.
    // Replaces the old "‹ value ›" arrow chrome — selection is now expressed
    // by the left bar + ACCENT, the same way every other list in z does it.
    let picker_row = |label: &str, value: &str, focused: bool| -> Line<'static> {
        let indicator = if focused { "\u{2502} " } else { "  " };
        let indicator_style = if focused {
            Style::default().fg(ACCENT)
        } else {
            Style::default()
        };
        let label_style = if focused {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(DIM)
        };
        let value_style = if focused {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(TEXT)
        };
```

Replace with:

```rust
    // Picker row: "│ Label    value" when focused, "  Label    value" when not.
    // Selection is encoded by the left bar plus brightness contrast on the
    // label (TEXT focused, DIM unfocused). Value text stays TEXT regardless —
    // it's content, not a focus indicator.
    let picker_row = |label: &str, value: &str, focused: bool| -> Line<'static> {
        let indicator = if focused { "\u{2502} " } else { "  " };
        let indicator_style = if focused {
            Style::default().fg(TEXT)
        } else {
            Style::default()
        };
        let label_style = if focused {
            Style::default().fg(TEXT)
        } else {
            Style::default().fg(DIM)
        };
        let value_style = Style::default().fg(TEXT);
```

(The rest of `picker_row` — the `Line::from(vec![...])` construction at lines 406-411 — is unchanged.)

- [ ] **Step 4.2: Verify build + tests**

Run: `cargo build && cargo test`
Expected: clean build, all tests pass.

- [ ] **Step 4.3: Manual verify**

Run: `cargo run`, press `n` to open the New Agent modal. Tab through Agent → Repo → Branch toggle.
Expected: the focused row shows `│` and label in `TEXT` brightness; unfocused rows show two spaces and DIM label. Values stay `TEXT` everywhere. No cyan.

Press `q` or `esc` to close, then `q` to quit.

- [ ] **Step 4.4: Commit**

```bash
git add src/ui.rs
git commit -m "Picker rows: TEXT brightness for focus, no cyan"
```

---

## Task 5: Migrate branch list selection

**Files:**
- Modify: `src/ui.rs:457-471` (inside `draw_new_agent_modal`'s branch list block)

The branch list has a three-way style: focused-and-selected (ACCENT), unfocused-but-selected (TEXT), unselected (DIM). With ACCENT gone, "focused and selected" collapses into the same `TEXT` as "selected"; the focus signal is carried by the surrounding modal context (focused field gets the `│` indicator + label).

- [ ] **Step 5.1: Replace the three-way style with a two-way TEXT/DIM**

Locate lines 457-471:

```rust
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
```

Replace with:

```rust
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
```

The `is_list` local stays in scope for use by the hint bar at lines 535+; only the branch-list inner block stops branching on it.

- [ ] **Step 5.2: Verify build + tests**

Run: `cargo build && cargo test`
Expected: clean build, all tests pass. (`is_list` may produce a "value assigned but never read" warning if it's no longer referenced anywhere else — check; if so, leave as `let _is_list = ...` or remove if truly unused. Per a quick scan, `is_list` is referenced only in this block, so remove the `let is_list = matches!(focus, NewAgentFocus::BranchList);` binding at line 434.)

If `is_list` is now unused, also remove its declaration (line 434):

```rust
    let is_list = matches!(focus, NewAgentFocus::BranchList);
```

Re-run: `cargo build && cargo test`
Expected: clean build, no warnings, all tests pass.

- [ ] **Step 5.3: Manual verify**

Run: `cargo run`, press `n`, tab to the Branch toggle, press `Enter`/arrows as needed to land on the branch list. Move with `j`/`k`.
Expected: the highlighted branch shows `│` and TEXT brightness; others are DIM. No cyan, no three-way distinction.

Quit.

- [ ] **Step 5.4: Commit**

```bash
git add src/ui.rs
git commit -m "Branch list: two-way TEXT/DIM, drop focus distinction"
```

---

## Task 6: Migrate prompt textarea body and synthetic cursor

**Files:**
- Modify: `src/ui.rs:506-531` (inside `draw_new_agent_modal`'s prompt rendering)

Two changes per spec: (a) the `_` cursor in the empty-prompt placeholder goes from ACCENT to TEXT, (b) the prompt textarea body's color stops varying with `is_prompt` — always TEXT.

- [ ] **Step 6.1: Replace ACCENT on the empty-prompt cursor**

Locate lines 506-518:

```rust
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
```

Replace the `ACCENT` line with `TEXT`:

```rust
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
```

- [ ] **Step 6.2: Drop the mode-conditional color on the typed-prompt body**

Locate lines 519-531 (the `else` branch of the same `if prompt.is_empty()`):

```rust
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
```

Replace the `.style(...)` line:

```rust
        let paragraph = Paragraph::new(text)
            .style(Style::default().fg(TEXT))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0));
```

(The `cursor = if is_prompt { "_" } else { "" }` stays — the `_` glyph's *presence* is the focus signal, exactly as the spec calls for.)

- [ ] **Step 6.3: Verify build + tests**

Run: `cargo build && cargo test`
Expected: clean build, all tests pass.

- [ ] **Step 6.4: Manual verify**

Run: `cargo run`, press `n`, tab all the way to the Prompt field, type a few characters, then tab away.
Expected: the typed prompt body renders in `TEXT` (white/default) regardless of whether you're in the prompt field or not. The trailing `_` cursor appears only when the prompt is focused, in `TEXT`. No cyan.

Quit.

- [ ] **Step 6.5: Commit**

```bash
git add src/ui.rs
git commit -m "Prompt: TEXT body always, _ cursor as focus signal"
```

---

## Task 7: Migrate delete modal inline name highlight

**Files:**
- Modify: `src/ui.rs:588-592` (inside `draw_delete_modal`)

Per the spec, the agent name in `Delete <name>?` becomes bold-TEXT (inline noun emphasis), distinct from modal-title bold and footer-key bold by context.

- [ ] **Step 7.1: Replace ACCENT with bold TEXT on the agent name**

Locate lines 588-592:

```rust
    let msg2 = Line::from(vec![
        Span::styled("  ", Style::default().fg(TEXT)),
        Span::styled(name, Style::default().fg(ACCENT)),
        Span::styled("?", Style::default().fg(TEXT)),
    ]);
```

Replace with:

```rust
    let msg2 = Line::from(vec![
        Span::styled("  ", Style::default().fg(TEXT)),
        Span::styled(name, Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
        Span::styled("?", Style::default().fg(TEXT)),
    ]);
```

- [ ] **Step 7.2: Verify build + tests**

Run: `cargo build && cargo test`
Expected: clean build, all tests pass.

- [ ] **Step 7.3: Manual verify**

Run: `cargo run`. With at least one agent in the list, press `d` to open the delete confirm.
Expected: the agent name in `Delete <name>?` reads as bold TEXT (no cyan); the surrounding `Delete worktree and branch for` and `?` stay TEXT non-bold. Press `esc` to cancel, then `q` to quit.

- [ ] **Step 7.4: Commit**

```bash
git add src/ui.rs
git commit -m "Delete modal: bold TEXT for agent name, no cyan"
```

---

## Task 8: Sweep ui.rs for any remaining ACCENT references

**Why:** Belt-and-suspenders before removing the constant. If a `grep` turns up nothing, Task 9 is a clean delete; if something was missed, this catches it before the compile-error gate.

**Files:**
- Inspect: `src/ui.rs`

- [ ] **Step 8.1: Grep for ACCENT in ui.rs**

Run: `grep -n ACCENT src/ui.rs`
Expected: only the import line at the top (`use crate::style::{ACCENT, ...}`). No other matches.

If any other matches appear, migrate them inline using the patterns from Tasks 1-7 (most likely target: `Style::default().fg(TEXT)` for selection/focus, `Style::default().fg(TEXT).add_modifier(Modifier::BOLD)` for inline noun emphasis), then re-run the grep.

- [ ] **Step 8.2: If any migrations happened in 8.1, commit them**

```bash
git add src/ui.rs
git commit -m "Sweep: migrate remaining ACCENT callsites"
```

If grep was already clean, skip the commit.

---

## Task 9: Remove ACCENT constant; update style.rs tests and module doc

**Why last:** Compile-gate. If any callsite still references `ACCENT`, the build fails and we know where to fix.

**Files:**
- Modify: `src/style.rs:1-17` (module doc + tokens)
- Modify: `src/style.rs:64-77` (tokens_have_expected_colors test)
- Modify: `src/style.rs:149-166` (modal_title and drift_arrow tests)
- Modify: `src/ui.rs:11` (import line)

- [ ] **Step 9.1: Rewrite the module doc**

Locate `src/style.rs:1-6`:

```rust
//! Design tokens and render helpers for z's TUI.
//!
//! Six semantic colors. Status colors (OK/BUSY/FAIL) appear only on agent
//! status indicators — never in chrome. ACCENT means "current selection or
//! focus" — never status, never decoration.
```

Replace with:

```rust
//! Design tokens and render helpers for z's TUI.
//!
//! Five semantic colors. Status colors (OK/BUSY/FAIL) appear only on agent
//! status indicators — never in chrome. Selection and focus are encoded
//! monochromatically: brightness contrast (TEXT vs DIM) plus structural
//! glyphs like the `│` left bar. Color is reserved for status meaning.
```

- [ ] **Step 9.2: Remove the ACCENT constant**

Locate `src/style.rs:12-17`:

```rust
pub const TEXT: Color = Color::Reset;
pub const DIM: Color = Color::DarkGray;
pub const ACCENT: Color = Color::Cyan;
pub const OK: Color = Color::Green;
pub const BUSY: Color = Color::Yellow;
pub const FAIL: Color = Color::Red;
```

Delete the `ACCENT` line:

```rust
pub const TEXT: Color = Color::Reset;
pub const DIM: Color = Color::DarkGray;
pub const OK: Color = Color::Green;
pub const BUSY: Color = Color::Yellow;
pub const FAIL: Color = Color::Red;
```

- [ ] **Step 9.3: Update the import in ui.rs**

Locate `src/ui.rs:11`:

```rust
use crate::style::{ACCENT, DIM, TEXT, drift_arrow, footer_hint, modal_title, status_color};
```

Remove `ACCENT`:

```rust
use crate::style::{DIM, TEXT, drift_arrow, footer_hint, modal_title, status_color};
```

- [ ] **Step 9.4: Verify the build now compiles**

Run: `cargo build`
Expected: clean build. If there's any "cannot find value `ACCENT`" error, return to Task 8 and migrate the missed callsite, then retry.

- [ ] **Step 9.5: Update tokens_have_expected_colors test**

Locate `src/style.rs:69-77`:

```rust
    #[test]
    fn tokens_have_expected_colors() {
        assert_eq!(TEXT, Color::Reset);
        assert_eq!(DIM, Color::DarkGray);
        assert_eq!(ACCENT, Color::Cyan);
        assert_eq!(OK, Color::Green);
        assert_eq!(BUSY, Color::Yellow);
        assert_eq!(FAIL, Color::Red);
    }
```

Remove the `ACCENT` assertion:

```rust
    #[test]
    fn tokens_have_expected_colors() {
        assert_eq!(TEXT, Color::Reset);
        assert_eq!(DIM, Color::DarkGray);
        assert_eq!(OK, Color::Green);
        assert_eq!(BUSY, Color::Yellow);
        assert_eq!(FAIL, Color::Red);
    }
```

- [ ] **Step 9.6: Update drift_arrow_is_dim_not_busy test**

Locate `src/style.rs:149-155`:

```rust
    #[test]
    fn drift_arrow_is_dim_not_busy() {
        let span = drift_arrow();
        assert_eq!(span.content, " \u{2192} ");
        assert_eq!(span.style, Style::default().fg(DIM));
        assert_ne!(span.style.fg, Some(BUSY));
    }
```

The `assert_ne!` against `BUSY` is the existing belt-and-suspenders check; it's still valid (BUSY exists). No change required — but rename the test to drop the `_not_busy` suffix is optional cleanup. **Leave as-is.** No edit in this step.

- [ ] **Step 9.7: Update modal_title_is_bold_text_not_accent test**

Locate `src/style.rs:157-166`:

```rust
    #[test]
    fn modal_title_is_bold_text_not_accent() {
        let span = modal_title("New Agent");
        assert_eq!(span.content, " New Agent ");
        assert_eq!(
            span.style,
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        );
        assert_ne!(span.style.fg, Some(ACCENT));
    }
```

The trailing `assert_ne!` against `ACCENT` no longer compiles. Remove it and rename the test for clarity:

```rust
    #[test]
    fn modal_title_is_bold_text() {
        let span = modal_title("New Agent");
        assert_eq!(span.content, " New Agent ");
        assert_eq!(
            span.style,
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        );
    }
```

- [ ] **Step 9.8: Verify all tests pass**

Run: `cargo test`
Expected: all tests pass. The five-color `tokens_have_expected_colors` passes; the renamed `modal_title_is_bold_text` passes; everything else is unchanged.

- [ ] **Step 9.9: Final manual verify**

Run: `cargo run`. Walk through all the surfaces touched in Tasks 1-7:
- agent table selection (arrow keys to move)
- separator branch label (when an agent is selected)
- New Agent modal: `n` to open, tab through Agent / Repo / Branch toggle / Branch list / Name / Prompt
- Delete modal: `d` to open

Expected: zero cyan anywhere. Status colors (green/yellow/red) still on agent dots and agent-table glyphs. Press `esc` and `q` to close modals and quit.

- [ ] **Step 9.10: Commit**

```bash
git add src/style.rs src/ui.rs
git commit -m "Remove ACCENT: monochrome chrome, status-only color"
```

---

## Self-Review

**Spec coverage check:**

| Spec section | Plan task |
|---|---|
| Remove `ACCENT` constant | Task 9 |
| Module doc rewrite | Task 9.1 |
| Agent table selected row | Task 1 |
| Separator branch label | Task 2 |
| Modal field label/value (focus state) | Task 3 |
| Picker row indicator/label/value | Task 4 |
| Modal list selected item | Task 5 |
| Branch list selection in New Agent modal | Task 5 |
| Prompt textarea body (always TEXT) | Task 6.2 |
| Prompt synthetic `_` cursor (TEXT) | Task 6.1 |
| Delete modal inline agent name (bold TEXT) | Task 7 |
| Status glyphs unchanged | (no task — verified by spec out-of-scope and manual run) |
| Test: drop ACCENT assertion in `tokens_have_expected_colors` | Task 9.5 |
| Test: rewrite `modal_title_is_bold_text_not_accent` | Task 9.7 |
| Test: rewrite `drift_arrow_is_dim_not_busy` | Task 9.6 (no edit — assertion still compiles) |

The spec also lists two "new regression tests" — agent-table indicator fg = TEXT, prompt body fg = TEXT regardless of mode. These are not separate tasks because removing the `ACCENT` constant in Task 9 is a stronger guarantee: the compile fails if any code references it. We don't need brittle string-content tests on inline render code when the type system already enforces what we want.

**Out-of-scope items honored:**
- No retuning of status-color shades.
- No reverse-video selection.
- No bold on selected row text.
- No animations / blink modifiers.
- No changes to `status_glyph` or `status_color()`.

**Anti-pattern check:**
- No "TBD" / "TODO" / placeholder language.
- Each step shows actual code, not "fix the colors here."
- Function and constant names match across tasks (`ACCENT`, `TEXT`, `DIM`, `picker_row`, `is_list`).
- Line-number citations match the file as inspected during plan writing.

**Type/name consistency:**
- `Style::default().fg(TEXT)` used uniformly for the selection/focus replacement.
- `Style::default().fg(TEXT).add_modifier(Modifier::BOLD)` only for the delete-modal name (Task 7).
- `Modifier` is already imported in `src/ui.rs:4`; no new import needed.

Plan looks consistent with the spec. Ready to execute.
