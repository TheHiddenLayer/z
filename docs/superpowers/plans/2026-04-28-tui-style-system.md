# TUI Style System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land a coherent design system across z's ratatui TUI that resolves the audit findings: a named token palette, one footer-hint contract, one selection metaphor, color-discipline (status colors stay on agent indicators only), and verb hygiene.

**Architecture:** Introduce `src/style.rs` as the design-token + render-helper module. Migrate every render site in `src/ui.rs` and key bindings in `src/app.rs` to consume the helpers. No new ratatui widgets, no theming framework — keep the surface area to six color tokens, three render helpers (`status_color`, `footer_hint`, `modal_title`), and one extracted `drift_arrow` span. Build only what today's three footer call sites and four modal field rows actually need; resist generalizing beyond that.

**Tech Stack:** Rust 2024, ratatui 0.29, crossterm 0.28. Tests are plain `#[cfg(test)]` units in each module — no `insta`, no snapshot harness. Visual-only changes get a manual verification step against `cargo run`.

**Audit reference:** Findings that drive each task are summarized in the conversation that produced this plan; cite ui.rs:NNN cites match the line numbers in the audit reports.

---

## File Structure

| File | Status | Responsibility |
|------|--------|----------------|
| `src/style.rs` | **create** | Color tokens, `status_color`, `footer_hint`, `modal_title`, `drift_arrow` |
| `src/ui.rs` | modify | Drop local `TEXT/DIM/FOCUS/READY/BUSY/FAIL` consts; import from `style`. Replace inline arrow chrome and footer literals with helpers. Apply selection metaphor uniformly. |
| `src/app.rs` | modify | Add `j/k` bindings in `NewAgent::BranchList`; add `q` alias for `CancelMode` in modals. New tests for those bindings. |
| `src/main.rs` | modify | Add `mod style;` declaration. |
| `Cargo.toml` | unchanged | No new deps. |

---

## Task 1: Extract `style.rs` with named tokens

**Why first:** Every later task references these constants. This is a mechanical rename — zero visual change, zero behavior change — so it's the safest possible foundation commit.

**Files:**
- Create: `src/style.rs`
- Modify: `src/main.rs` (add `mod style;`)
- Modify: `src/ui.rs:1-18` (drop local consts, import from `style`)

**Note on rename:** The audit recommended `accent` (semantic) over `FOCUS` (mechanical). We rename in this task. `READY` becomes `OK` for symmetry with `FAIL`.

- [ ] **Step 1.1: Write failing tests for token values**

Create `src/style.rs` with only the tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn tokens_have_expected_colors() {
        assert_eq!(TEXT, Color::Reset);
        assert_eq!(DIM, Color::DarkGray);
        assert_eq!(ACCENT, Color::Cyan);
        assert_eq!(OK, Color::Green);
        assert_eq!(BUSY, Color::Yellow);
        assert_eq!(FAIL, Color::Red);
    }
}
```

- [ ] **Step 1.2: Run test to verify it fails**

Run: `cargo test --lib style::tests::tokens_have_expected_colors`
Expected: compile error — symbols not defined.

- [ ] **Step 1.3: Implement tokens**

Replace `src/style.rs` contents with:

```rust
//! Design tokens and render helpers for z's TUI.
//!
//! Six semantic colors. Status colors (OK/BUSY/FAIL) appear only on agent
//! status indicators — never in chrome. ACCENT means "current selection or
//! focus" — never status, never decoration.

use ratatui::style::Color;

pub const TEXT: Color = Color::Reset;
pub const DIM: Color = Color::DarkGray;
pub const ACCENT: Color = Color::Cyan;
pub const OK: Color = Color::Green;
pub const BUSY: Color = Color::Yellow;
pub const FAIL: Color = Color::Red;

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn tokens_have_expected_colors() {
        assert_eq!(TEXT, Color::Reset);
        assert_eq!(DIM, Color::DarkGray);
        assert_eq!(ACCENT, Color::Cyan);
        assert_eq!(OK, Color::Green);
        assert_eq!(BUSY, Color::Yellow);
        assert_eq!(FAIL, Color::Red);
    }
}
```

- [ ] **Step 1.4: Wire module into the crate**

In `src/main.rs`, find the existing `mod` declarations (alongside `mod app;`, `mod ui;`, etc.) and add:

```rust
mod style;
```

- [ ] **Step 1.5: Run tests to verify they pass**

Run: `cargo test --lib style::`
Expected: PASS.

- [ ] **Step 1.6: Migrate `ui.rs` to import tokens**

Replace `src/ui.rs:11-18`:

```rust
// Strategic palette — every color carries one meaning, and we lean on the
// terminal's own theme rather than overriding it. No backgrounds anywhere.
const TEXT: Color = Color::Reset;     // primary content; honors terminal fg
const DIM: Color = Color::DarkGray;   // metadata, labels, separators, hints
const FOCUS: Color = Color::Cyan;     // selection / focused field / modal accent
const READY: Color = Color::Green;    // ✓ — session alive and quiet
const BUSY: Color = Color::Yellow;    // spinner + slug → branch drift (in-flight / unsettled)
const FAIL: Color = Color::Red;       // error glyph
```

with:

```rust
use crate::style::{ACCENT, BUSY, DIM, FAIL, OK, TEXT};
```

Then in `src/ui.rs`, replace every occurrence of `FOCUS` with `ACCENT` and every occurrence of `READY` with `OK`. (Editor: do this two `sed`/replace_all passes — `FOCUS` → `ACCENT`, then `READY` → `OK`. Both names are local to ui.rs after the import line is in place.)

- [ ] **Step 1.7: Verify build & existing tests pass**

Run: `cargo build && cargo test`
Expected: clean build, all existing tests pass. No visual change in the TUI.

- [ ] **Step 1.8: Manual TUI sanity**

Run: `cargo run`
Expected: TUI looks exactly as before — selected agent rows still cyan, status glyphs still green/yellow/red, drift arrow still yellow, separator dim, etc. Quit with `q`.

- [ ] **Step 1.9: Commit**

```bash
git add src/style.rs src/main.rs src/ui.rs
git commit -m "Extract style tokens into dedicated module"
```

---

## Task 2: Move `status_color` into `style.rs`

**Why:** `status_color` is the single point that maps an agent state → a status color. It belongs with the tokens, not in `ui.rs`. Moving it makes future re-use (e.g. an HTTP debug surface) trivial and makes the agent-state→color mapping unit-testable without touching rendering.

**Files:**
- Modify: `src/style.rs` (add `status_color` + tests)
- Modify: `src/ui.rs:84-91` (delete local `status_color`; import from `style`)

- [ ] **Step 2.1: Promote the existing test helper**

There is already a private test helper `make_agent_with_status` at `src/agent.rs:1019`. Promote it to `pub(crate)` so `style.rs::tests` can use it. Change `src/agent.rs:1019`:

```rust
    fn make_agent_with_status(status: AgentStatus) -> Agent {
```

to:

```rust
    pub(crate) fn make_agent_with_status(status: AgentStatus) -> Agent {
```

The helper is inside a `#[cfg(test)] mod tests` block, so `pub(crate)` only widens visibility within test builds — production code is unaffected.

- [ ] **Step 2.2: Write failing tests in `style.rs`**

Append to `src/style.rs` `tests` module:

```rust
    use crate::agent::{AgentStatus, tests::make_agent_with_status};

    #[test]
    fn status_color_error_is_fail() {
        let a = make_agent_with_status(AgentStatus::Error("boom".into()));
        assert_eq!(status_color(&a), FAIL);
    }

    #[test]
    fn status_color_stopped_is_dim() {
        let a = make_agent_with_status(AgentStatus::Stopped);
        assert_eq!(status_color(&a), DIM);
    }

    #[test]
    fn status_color_creating_is_busy() {
        // shows_spinner() is unconditionally true for Creating (agent.rs:125).
        let a = make_agent_with_status(AgentStatus::Creating);
        assert_eq!(status_color(&a), BUSY);
    }

    #[test]
    fn status_color_idle_running_is_ok() {
        // Running + last_pane_hash=None + was_spinner_visible=false → idle
        // (see shows_spinner_follows_was_spinner_visible_when_hash_cleared in agent.rs).
        let mut a = make_agent_with_status(AgentStatus::Running);
        a.last_pane_hash = None;
        a.was_spinner_visible = false;
        assert!(!a.shows_spinner());
        assert_eq!(status_color(&a), OK);
    }
```

To re-export the test helper for external test modules, also add at the top of the `tests` module in `src/agent.rs` (or wherever `mod tests` is declared) a `pub(crate)` on the module if it isn't already public to the crate. Concretely, in `src/agent.rs` find `mod tests {` and change it to `pub(crate) mod tests {`. (If it's already `pub(crate) mod tests`, skip.)

- [ ] **Step 2.3: Run failing tests**

Run: `cargo test --lib style::`
Expected: FAIL — `status_color` undefined.

- [ ] **Step 2.4: Implement `status_color` in `style.rs`**

Add to `src/style.rs` (above the `tests` module):

```rust
use crate::agent::{Agent, AgentStatus};

pub fn status_color(agent: &Agent) -> Color {
    match &agent.status {
        AgentStatus::Error(_) => FAIL,
        AgentStatus::Stopped => DIM,
        _ if agent.shows_spinner() => BUSY,
        _ => OK,
    }
}
```

- [ ] **Step 2.5: Delete the duplicate in `ui.rs`**

Remove `src/ui.rs:84-91` (the local `status_color` fn) and add `status_color` to the existing import from `crate::style`:

```rust
use crate::style::{ACCENT, BUSY, DIM, FAIL, OK, TEXT, status_color};
```

- [ ] **Step 2.6: Run all tests**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 2.7: Commit**

```bash
git add src/style.rs src/ui.rs src/agent.rs
git commit -m "Move status_color to style module"
```

---

## Task 3: Add `footer_hint` helper

**Why:** The audit found three incompatible footer formats (`ui.rs:332`, `ui.rs:559`, `ui.rs:626`) — different separators, different key styling, different escape paths. One helper produces all of them so the contract is enforced by code, not discipline.

**Files:**
- Modify: `src/style.rs` (add `footer_hint` + tests)

- [ ] **Step 3.1: Write failing tests**

Append to `src/style.rs` `tests` module:

```rust
    use ratatui::style::Modifier;

    #[test]
    fn footer_hint_renders_single_pair() {
        let line = footer_hint(&[("q", "quit")]);
        // 2 spans: bold key, dim label
        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].content, "q");
        assert_eq!(
            line.spans[0].style,
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        );
        assert_eq!(line.spans[1].content, " quit");
        assert_eq!(line.spans[1].style, Style::default().fg(DIM));
    }

    #[test]
    fn footer_hint_inserts_bullet_separator_between_pairs() {
        let line = footer_hint(&[("q", "quit"), ("?", "help")]);
        // 5 spans: key, label, sep, key, label
        assert_eq!(line.spans.len(), 5);
        assert_eq!(line.spans[2].content, " \u{00b7} ");
        assert_eq!(line.spans[2].style, Style::default().fg(DIM));
    }

    #[test]
    fn footer_hint_no_trailing_separator() {
        let line = footer_hint(&[("a", "b"), ("c", "d")]);
        let last = line.spans.last().unwrap();
        assert_eq!(last.content, " d");
    }

    #[test]
    fn footer_hint_empty_input_yields_empty_line() {
        let line = footer_hint(&[]);
        assert!(line.spans.is_empty());
    }
```

- [ ] **Step 3.2: Run failing tests**

Run: `cargo test --lib style::tests::footer_hint`
Expected: FAIL — `footer_hint` undefined.

- [ ] **Step 3.3: Implement `footer_hint`**

Add to `src/style.rs` (with the other helpers, above `tests`):

```rust
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// Build a footer hint line: bold key + dim label, repeated, separated by ` · `.
///
/// One contract for every screen: keys are bold and at terminal-text brightness,
/// labels are dim, separator is a middle dot. Pairs are `(key, label)` so authors
/// keep the binding glyph and its verb visually adjacent in source.
pub fn footer_hint(items: &[(&str, &str)]) -> Line<'static> {
    let key_style = Style::default().fg(TEXT).add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(DIM);
    let sep = " \u{00b7} ";
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, (key, label)) in items.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(sep, label_style));
        }
        spans.push(Span::styled((*key).to_string(), key_style));
        spans.push(Span::styled(format!(" {label}"), label_style));
    }
    Line::from(spans)
}
```

- [ ] **Step 3.4: Run tests**

Run: `cargo test --lib style::tests::footer_hint`
Expected: PASS (all four tests).

- [ ] **Step 3.5: Commit**

```bash
git add src/style.rs
git commit -m "Add footer_hint helper"
```

---

## Task 4: Migrate the main status bar to `footer_hint`

**Why:** This is the easiest call site (matches `footer_hint`'s default contract exactly: bold keys, dim labels, ` · ` separator). Migrating it first proves the helper round-trips with zero visual change.

**Files:**
- Modify: `src/ui.rs:332-358` (`draw_status_bar`)

- [ ] **Step 4.1: Replace the hint construction**

Change `src/ui.rs:332-358` from:

```rust
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
```

to:

```rust
fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let line = if let Some(msg) = &app.status_message {
        Line::from(Span::styled(msg.as_str(), Style::default().fg(DIM)))
    } else {
        footer_hint(&[
            ("↑/k", "up"),
            ("↓/j", "down"),
            ("n", "new"),
            ("a", "attach"),
            ("x", "stop"),
            ("d", "delete"),
            ("q", "quit"),
        ])
    };
    frame.render_widget(Paragraph::new(line), area);
}
```

Note the deliberate additions: `↑/k up` and `↓/j down` come *first* — the audit found these were missing despite j/k being bound at `app.rs:885-886`. The Charm reference puts navigation first.

Add `footer_hint` to the `style` import at top of `src/ui.rs`:

```rust
use crate::style::{ACCENT, BUSY, DIM, FAIL, OK, TEXT, footer_hint, status_color};
```

- [ ] **Step 4.2: Build and run tests**

Run: `cargo build && cargo test`
Expected: PASS.

- [ ] **Step 4.3: Manual verification**

Run: `cargo run`
Expected: Status bar reads `↑/k up · ↓/j down · n new · a attach · x stop · d delete · q quit`. Keys are bold and bright; labels and separators are dim. Trigger a status message (e.g. by attempting an action that produces one) and confirm the bar still falls back to the dim message.

- [ ] **Step 4.4: Commit**

```bash
git add src/ui.rs
git commit -m "Render main status bar via footer_hint helper"
```

---

## Task 5: Migrate the New Agent modal hints to `footer_hint`

**Why:** The four context-specific hint variants at `ui.rs:557-575` render the entire string in dim — keys get no emphasis. Routing them through `footer_hint` lifts the keys to bold-text and unifies separator handling.

**Files:**
- Modify: `src/ui.rs:556-575`

- [ ] **Step 5.1: Replace the hint match**

Change `src/ui.rs:556-575` from:

```rust
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
```

to:

```rust
    // --- Hint bar ---
    let hint_line = match focus {
        NewAgentFocus::Agent | NewAgentFocus::Repo | NewAgentFocus::BranchToggle => {
            footer_hint(&[("←/→", "cycle"), ("tab", "next"), ("esc", "cancel")])
        }
        NewAgentFocus::BranchList => {
            footer_hint(&[("↑/k", "up"), ("↓/j", "down"), ("tab", "next"), ("esc", "cancel")])
        }
        NewAgentFocus::Name => {
            footer_hint(&[("tab", "next"), ("esc", "cancel")])
        }
        NewAgentFocus::Prompt => {
            footer_hint(&[
                ("enter", "start"),
                ("alt+enter", "newline"),
                ("tab", "options"),
                ("esc", "cancel"),
            ])
        }
    };
    // Indent the hint line under the form's value column for visual continuity.
    let mut spans = vec![Span::raw(" ".repeat(label_w as usize))];
    spans.extend(hint_line.spans);
    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[11]);
```

Note: `BranchList` advertises `↑/k up · ↓/j down` — j/k will be wired into `app.rs` for that focus in **Task 7**. The hint is correct now and starts working after Task 7 lands; advertising before binding is fine because the hint matches the intent.

- [ ] **Step 5.2: Build and run tests**

Run: `cargo build && cargo test`
Expected: PASS.

- [ ] **Step 5.3: Manual verification**

Run: `cargo run`, press `n` to open the New Agent modal. Tab through Agent → Repo → BranchToggle → BranchList → Name → Prompt and confirm:
- Each hint bar uses bold keys (`tab`, `esc`, `enter`, `←/→`, `↑/j`, `↓/k`) on bright text.
- Separator is ` · ` (middle dot, padded with single spaces).
- Indentation matches the form values column (no left-shift).

- [ ] **Step 5.4: Commit**

```bash
git add src/ui.rs
git commit -m "Render New Agent modal hints via footer_hint helper"
```

---

## Task 6: Migrate the Delete modal hints + verb hygiene

**Why:** The delete modal uses cyan-on-keys + double-space separator (`ui.rs:626-644`) — both diverge from the contract. It also says "y confirm" when no session exists; `confirm` is generic boilerplate, the actual verb is `delete`.

**Files:**
- Modify: `src/ui.rs:626-644`

- [ ] **Step 6.1: Replace both hint branches**

Change `src/ui.rs:626-644` from:

```rust
    let hint = if has_session {
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("y", Style::default().fg(FOCUS)),
            Span::styled(" delete + tmux  ", Style::default().fg(DIM)),
            Span::styled("p", Style::default().fg(FOCUS)),
            Span::styled(" preserve tmux  ", Style::default().fg(DIM)),
            Span::styled("esc", Style::default().fg(FOCUS)),
            Span::styled(" cancel", Style::default().fg(DIM)),
        ])
    } else {
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("y", Style::default().fg(FOCUS)),
            Span::styled(" confirm  ", Style::default().fg(DIM)),
            Span::styled("esc", Style::default().fg(FOCUS)),
            Span::styled(" cancel", Style::default().fg(DIM)),
        ])
    };
    frame.render_widget(Paragraph::new(hint), chunks[5]);
```

to:

```rust
    let hint = if has_session {
        footer_hint(&[
            ("y", "delete + tmux"),
            ("p", "preserve tmux"),
            ("esc", "cancel"),
        ])
    } else {
        footer_hint(&[("y", "delete"), ("esc", "cancel")])
    };
    let mut spans = vec![Span::raw("  ")];
    spans.extend(hint.spans);
    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[5]);
```

The `"  "` prefix preserves the modal's internal padding column.

- [ ] **Step 6.2: Build and run tests**

Run: `cargo build && cargo test`
Expected: PASS.

- [ ] **Step 6.3: Manual verification**

Run: `cargo run`, create an agent (or attach to one), then press `d` to open the delete modal:
- With a live tmux session: hint reads `y delete + tmux · p preserve tmux · esc cancel`. Keys bold, labels dim, separator ` · `.
- Without a session: hint reads `y delete · esc cancel` (no longer "confirm").

- [ ] **Step 6.4: Commit**

```bash
git add src/ui.rs
git commit -m "Render delete modal hints via footer_hint and replace 'confirm' with 'delete'"
```

---

## Task 7: Add `j`/`k` bindings inside the New Agent branch list

**Why:** The hint added in Task 5 advertises `↑/k up · ↓/j down`, but `app.rs:903-933` only handles arrow keys for `NewAgentFocus::BranchList`. j/k chars currently do nothing in that focus (they don't match the `Prompt|Name` `TypeChar` arm at `app.rs:932`). Wire them up.

**Files:**
- Modify: `src/app.rs:903-933` (NewAgent key handling)
- Modify: `src/app.rs` test module (add a binding test)

- [ ] **Step 7.1: Read the existing pattern**

Open `src/app.rs:903-933`. Note the pattern: arms are guarded by `matches!(focus, ...)` for focus-specific bindings.

- [ ] **Step 7.2: Write failing tests**

The test module starts at `src/app.rs:942` (`mod tests`) with helpers `test_app()` (line 946) and `make_key()` (line 1496). The existing pattern to enter NewAgent mode is `app.update(Action::StartNewAgent)` then `Action::FocusNext` to advance focus through the rows. The focus order is Agent → Repo → BranchToggle → BranchList → (Name) → Prompt, so three `FocusNext` calls land on `BranchList`.

Add to the `tests` module:

```rust
    #[test]
    fn newagent_branchlist_k_moves_up() {
        let mut app = test_app();
        app.update(Action::StartNewAgent);
        app.update(Action::FocusNext); // Repo
        app.update(Action::FocusNext); // BranchToggle
        app.update(Action::FocusNext); // BranchList
        let action = app.handle_key(make_key(KeyCode::Char('k')));
        assert!(matches!(action, Some(Action::PickerPrev)));
    }

    #[test]
    fn newagent_branchlist_j_moves_down() {
        let mut app = test_app();
        app.update(Action::StartNewAgent);
        app.update(Action::FocusNext);
        app.update(Action::FocusNext);
        app.update(Action::FocusNext); // BranchList
        let action = app.handle_key(make_key(KeyCode::Char('j')));
        assert!(matches!(action, Some(Action::PickerNext)));
    }

    #[test]
    fn newagent_prompt_j_still_types() {
        let mut app = test_app();
        app.update(Action::StartNewAgent);
        // Walk all the way to Prompt — count FocusNext calls to land there.
        // Order is Agent → Repo → BranchToggle → BranchList → Name → Prompt
        // (Name is skipped if BranchMode::Existing). Default is BranchMode::New.
        for _ in 0..5 {
            app.update(Action::FocusNext);
        }
        // Sanity: confirm we're on Prompt before testing the binding.
        if let Mode::NewAgent { focus, .. } = &app.mode {
            assert!(matches!(focus, NewAgentFocus::Prompt),
                "test setup: expected Prompt focus, got {focus:?}");
        }
        let action = app.handle_key(make_key(KeyCode::Char('j')));
        assert!(matches!(action, Some(Action::TypeChar('j'))));
    }
```

`Action::PickerNext`/`PickerPrev` are confirmed real (`src/app.rs:46-47`); `Action::FocusNext`/`FocusPrev` (`src/app.rs:51-52`) and `Action::CancelMode` (`src/app.rs:43`) are real too. If the focus walk in `newagent_prompt_j_still_types` lands somewhere other than Prompt (e.g. the default `BranchMode` differs from what `StartNewAgent` produces), adjust the loop count after running once and reading the assertion error — don't paper over it.

- [ ] **Step 7.3: Run tests to verify they fail**

Run: `cargo test --lib newagent_branchlist`
Expected: FAIL — `j` and `k` either return `None` or fall through to `TypeChar`.

- [ ] **Step 7.4: Add the bindings**

In `src/app.rs:903-933`, *before* the `KeyCode::Char(c) if matches!(focus, NewAgentFocus::Prompt | NewAgentFocus::Name)` arm, insert:

```rust
                KeyCode::Char('k') if matches!(focus, NewAgentFocus::BranchList) => {
                    Some(Action::PickerPrev)
                }
                KeyCode::Char('j') if matches!(focus, NewAgentFocus::BranchList) => {
                    Some(Action::PickerNext)
                }
```

(Substitute the actual `Picker*` action names you confirmed in step 7.2.)

- [ ] **Step 7.5: Run tests**

Run: `cargo test`
Expected: PASS — including `newagent_prompt_j_still_types`, which proves the new arm doesn't shadow `TypeChar` for Prompt focus.

- [ ] **Step 7.6: Manual verification**

Run: `cargo run`, press `n`, tab to BranchList, press `j` and `k` — selection should move down/up just like arrow keys.

- [ ] **Step 7.7: Commit**

```bash
git add src/app.rs
git commit -m "Bind j/k in New Agent branch list"
```

---

## Task 8: Add `q` alias for cancel in modals

**Why:** The audit found `q` is bound to Quit in Normal mode (`app.rs:884`) but only `Esc` cancels modals (`app.rs:898, 904`). Two-key escape is friendlier and matches the Charm convention of "every screen names how to leave it." Add `q` as an alias only where it doesn't collide with text input.

**Files:**
- Modify: `src/app.rs:897-907` (ConfirmDelete + NewAgent key handling)
- Modify: `src/app.rs` test module
- Modify: `src/ui.rs` delete-modal hint

- [ ] **Step 8.1: Write failing tests**

Add to the app.rs test module, using the same setup pattern as Task 7:

```rust
    #[test]
    fn confirmdelete_q_cancels() {
        let mut app = test_app();
        app.agents = vec![mock_agent("a")];
        app.update(Action::StartDelete);
        let action = app.handle_key(make_key(KeyCode::Char('q')));
        assert!(matches!(action, Some(Action::CancelMode)));
    }

    #[test]
    fn newagent_branchlist_q_cancels() {
        let mut app = test_app();
        app.update(Action::StartNewAgent);
        app.update(Action::FocusNext);
        app.update(Action::FocusNext);
        app.update(Action::FocusNext); // BranchList
        let action = app.handle_key(make_key(KeyCode::Char('q')));
        assert!(matches!(action, Some(Action::CancelMode)));
    }

    #[test]
    fn newagent_prompt_q_still_types() {
        let mut app = test_app();
        app.update(Action::StartNewAgent);
        for _ in 0..5 {
            app.update(Action::FocusNext);
        }
        // Sanity check focus is Prompt (see Task 7's note).
        if let Mode::NewAgent { focus, .. } = &app.mode {
            assert!(matches!(focus, NewAgentFocus::Prompt));
        }
        let action = app.handle_key(make_key(KeyCode::Char('q')));
        assert!(matches!(action, Some(Action::TypeChar('q'))));
    }
```

`mock_agent(...)` is the existing test helper used by other tests in the module (search for `mock_agent` in `src/app.rs` if its signature is unclear; it's used at e.g. `app.rs:962`).

- [ ] **Step 8.2: Run failing tests**

Run: `cargo test --lib q_`
Expected: FAIL.

- [ ] **Step 8.3: Bind `q` in modals**

In `src/app.rs:897-901` (ConfirmDelete arm), add:

```rust
                KeyCode::Char('q') => Some(Action::CancelMode),
```

In `src/app.rs:903-933` (NewAgent arm), add — placed *before* the `TypeChar` catch-all so it doesn't get consumed:

```rust
                KeyCode::Char('q') if !matches!(focus, NewAgentFocus::Prompt | NewAgentFocus::Name) => {
                    Some(Action::CancelMode)
                }
```

- [ ] **Step 8.4: Run tests**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 8.5: Update delete-modal hint to advertise `q`**

In `src/ui.rs` `draw_delete_modal`, change the hint pairs to include `q` next to `esc`:

```rust
    let hint = if has_session {
        footer_hint(&[
            ("y", "delete + tmux"),
            ("p", "preserve tmux"),
            ("q/esc", "cancel"),
        ])
    } else {
        footer_hint(&[("y", "delete"), ("q/esc", "cancel")])
    };
```

In `src/ui.rs` New Agent hints (Task 5 output), update the cancel pair on every variant *except* `Prompt` and `Name` (where `q` types literally) to `("q/esc", "cancel")`:

```rust
        NewAgentFocus::Agent | NewAgentFocus::Repo | NewAgentFocus::BranchToggle => {
            footer_hint(&[("←/→", "cycle"), ("tab", "next"), ("q/esc", "cancel")])
        }
        NewAgentFocus::BranchList => {
            footer_hint(&[("↑/k", "up"), ("↓/j", "down"), ("tab", "next"), ("q/esc", "cancel")])
        }
        NewAgentFocus::Name => {
            footer_hint(&[("tab", "next"), ("esc", "cancel")])  // q types literally here
        }
        NewAgentFocus::Prompt => {
            footer_hint(&[
                ("enter", "start"),
                ("alt+enter", "newline"),
                ("tab", "options"),
                ("esc", "cancel"),  // q types literally here
            ])
        }
```

- [ ] **Step 8.6: Manual verification**

Run: `cargo run`. From Normal mode, press `d` — confirm `q` dismisses the delete modal. Press `n`, tab into Repo focus, press `q` — confirm modal closes. Press `n`, tab to Prompt, type `q` — confirm `q` is inserted into the prompt buffer (modal stays open).

- [ ] **Step 8.7: Commit**

```bash
git add src/app.rs src/ui.rs
git commit -m "Add q as cancel alias in modals (where it doesn't collide with text input)"
```

---

## Task 9: Move the empty-state hint into the footer

**Why:** When no agents exist, `ui.rs:110-127` renders `"No agents running. Press n to create one."` *inside the agent table area*, which breaks the audit's "footer is the contract for affordances" rule. Body content describes state; the footer advertises actions.

**Files:**
- Modify: `src/ui.rs:110-127` (`draw_agent_table` empty branch)

- [ ] **Step 9.1: Simplify the empty-state body copy**

Change `src/ui.rs:110-127` from:

```rust
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
                Span::styled("n", Style::default().fg(FOCUS)),
                Span::styled(" to create one.", Style::default().fg(DIM)),
            ])
        };
        frame.render_widget(Paragraph::new(line), area);
        return;
    }
```

to:

```rust
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
```

The footer (rendered separately in `draw_status_bar`) already advertises `n new` — that's where users learn the affordance. The body just describes state.

- [ ] **Step 9.2: Build and test**

Run: `cargo build && cargo test`
Expected: PASS.

- [ ] **Step 9.3: Manual verification**

Run: `cargo run` with no existing agents (or kill the session/tmux state so the agent list is empty). Body should read `No agents yet.` in dim. Footer should still show `n new` among other bindings — that's where the user reads the affordance.

- [ ] **Step 9.4: Commit**

```bash
git add src/ui.rs
git commit -m "Move agent-creation hint from empty-state body to footer"
```

---

## Task 10: Strip status color (yellow) from the drift arrow

**Why:** The audit's central color finding: `BUSY` (yellow) carries two meanings — spinner animation *and* slug→branch drift. Same color, two semantics, visual collision when an agent drifts and is also working. Drift is a spatial/structural fact, not a status, so it loses the status color and uses `DIM`. The arrow glyph alone signals drift.

**Files:**
- Modify: `src/style.rs` (add `drift_arrow` helper + tests)
- Modify: `src/ui.rs:184-192, 236-244` (use the helper in table row + separator)

- [ ] **Step 10.1: Write failing test**

Append to `src/style.rs` `tests`:

```rust
    #[test]
    fn drift_arrow_is_dim_not_busy() {
        let span = drift_arrow();
        assert_eq!(span.content, " \u{2192} ");
        assert_eq!(span.style, Style::default().fg(DIM));
        assert_ne!(span.style.fg, Some(BUSY));
    }
```

- [ ] **Step 10.2: Run failing test**

Run: `cargo test --lib drift_arrow`
Expected: FAIL.

- [ ] **Step 10.3: Implement `drift_arrow`**

Add to `src/style.rs`:

```rust
/// The "→" between an agent's slug and its actual branch name when they
/// disagree. Drift is a structural fact, not a status — DIM, glyph carries
/// the meaning.
pub fn drift_arrow() -> Span<'static> {
    Span::styled(" \u{2192} ", Style::default().fg(DIM))
}
```

- [ ] **Step 10.4: Use it in the agent table row**

In `src/ui.rs:184-192`, change:

```rust
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
```

to:

```rust
        let drifted = agent.slug != agent.branch.replace('/', "-");
        let branch_cell = if drifted {
            Line::from(vec![
                Span::styled(agent.slug.as_str(), text_style),
                drift_arrow(),
                Span::styled(agent.branch.as_str(), text_style.add_modifier(Modifier::ITALIC)),
            ])
        } else {
            Line::from(Span::styled(agent.branch.as_str(), text_style))
        };
```

- [ ] **Step 10.5: Use it in the separator label**

In `src/ui.rs:236-244`, change:

```rust
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
```

to:

```rust
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

Add `drift_arrow` to the `style` import line at the top of `src/ui.rs`.

- [ ] **Step 10.6: Confirm `BUSY` is now used only by status indicators**

Run: `grep -n "BUSY" src/ui.rs`
Expected: matches only inside `status_color` callers (status glyph rendering — though `status_color` itself moved to `style.rs` in Task 2, so `ui.rs` may no longer reference `BUSY` at all). If there are any remaining BUSY uses in `ui.rs` chrome (separators, borders, decorations), that's a regression — fix it.

- [ ] **Step 10.7: Tests + manual verification**

Run: `cargo test`
Expected: PASS.

Run: `cargo run` with at least one agent whose `slug` differs from `branch.replace('/', "-")` (you may need to manually rename the worktree branch to force drift). Confirm:
- The `slug → branch` arrow in the agent table is dim, not yellow.
- The same arrow on the separator label is dim, not yellow.
- A spinning agent's spinner glyph remains yellow (status color survives untouched).

- [ ] **Step 10.8: Commit**

```bash
git add src/style.rs src/ui.rs
git commit -m "Drift arrow is a structural signal, not a status — render dim"
```

---

## Task 11: Strip accent color from modal titles

**Why:** Modal titles (`ui.rs:366` `New Agent`, `ui.rs:582` `Delete Agent`) render in `ACCENT` (cyan). ACCENT means "current selection or focus." A modal title is neither — it's an identifying header. Use bold text instead so the title is emphasized by weight, not by a color that's reserved for selection state.

**Files:**
- Modify: `src/style.rs` (add `modal_title` helper + test)
- Modify: `src/ui.rs:366, 582`

- [ ] **Step 11.1: Write failing test**

Append to `src/style.rs` `tests`:

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

- [ ] **Step 11.2: Run failing test**

Run: `cargo test --lib modal_title`
Expected: FAIL.

- [ ] **Step 11.3: Implement `modal_title`**

Add to `src/style.rs`:

```rust
/// Modal/dialog title span. Bold + TEXT — emphasis from weight, not color.
/// ACCENT is reserved for selection/focus and must not appear on chrome.
pub fn modal_title(text: &str) -> Span<'static> {
    Span::styled(
        format!(" {text} "),
        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
    )
}
```

- [ ] **Step 11.4: Use it in both modals**

In `src/ui.rs:363-366` (New Agent modal block):

```rust
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(modal_title("New Agent"));
```

In `src/ui.rs:579-582` (Delete modal block):

```rust
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(modal_title("Delete Agent"));
```

Add `modal_title` to the `style` import line at the top of `src/ui.rs`.

- [ ] **Step 11.5: Tests + manual verification**

Run: `cargo test`
Expected: PASS.

Run: `cargo run`. Open the New Agent modal (`n`) and the Delete modal (select an agent, press `d`). Both titles should render in bold terminal-text color (no cyan). Borders remain dim.

- [ ] **Step 11.6: Commit**

```bash
git add src/style.rs src/ui.rs
git commit -m "Modal titles use bold text; ACCENT is reserved for selection"
```

---

## Task 12: Unify selection metaphor in the New Agent modal

**Why:** The audit's biggest hierarchy finding: the New Agent modal uses `‹ value ›` arrow chrome (`ui.rs:419-453`) for cycling pickers (Agent, Repo, BranchToggle), while every other "current selection" surface in z uses `│ + ACCENT` (agent table at `ui.rs:165`, branch list at `ui.rs:481`). Two metaphors for one concept. Replace the arrow chrome with the standard left-bar pattern; keep ←/→ as the keybinding (advertised in the footer hint already updated in Tasks 5 & 8).

This is the most visible UX change in the plan.

**Files:**
- Modify: `src/ui.rs:416-453` (Agent, Repo, BranchToggle rows)

- [ ] **Step 12.1: Write the new picker-row helper**

Inside `src/ui.rs` (or `src/style.rs` if you prefer — but only if a second consumer materializes; otherwise YAGNI), add a private helper near the top of `draw_new_agent_modal`:

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
```

- [ ] **Step 12.2: Replace the Agent row**

Change `src/ui.rs:416-426` from:

```rust
    // --- Agent row ---
    let is_agent = matches!(focus, NewAgentFocus::Agent);
    let kind_label = agent_name.as_str();
    let agent_line = Line::from(vec![
        Span::styled("  Agent", label_style(is_agent)),
        Span::raw(" ".repeat((label_w as usize).saturating_sub(7))),
        Span::styled("\u{2039} ", Style::default().fg(if is_agent { FOCUS } else { DIM })),
        Span::styled(kind_label, val_style(is_agent)),
        Span::styled(" \u{203a}", Style::default().fg(if is_agent { FOCUS } else { DIM })),
    ]);
    frame.render_widget(Paragraph::new(agent_line), chunks[1]);
```

to:

```rust
    // --- Agent row ---
    let is_agent = matches!(focus, NewAgentFocus::Agent);
    let agent_line = picker_row("Agent", agent_name.as_str(), is_agent);
    frame.render_widget(Paragraph::new(agent_line), chunks[1]);
```

- [ ] **Step 12.3: Replace the Repo row**

Change `src/ui.rs:428-438` from:

```rust
    // --- Repo row ---
    let is_repo = matches!(focus, NewAgentFocus::Repo);
    let repo_arrows = if repos.len() > 1 { ("\u{2039} ", " \u{203a}") } else { ("", "") };
    let repo_line = Line::from(vec![
        Span::styled("  Repo", label_style(is_repo)),
        Span::raw(" ".repeat((label_w as usize).saturating_sub(6))),
        Span::styled(repo_arrows.0, Style::default().fg(if is_repo { FOCUS } else { DIM })),
        Span::styled(repo_name, val_style(is_repo)),
        Span::styled(repo_arrows.1, Style::default().fg(if is_repo { FOCUS } else { DIM })),
    ]);
    frame.render_widget(Paragraph::new(repo_line), chunks[3]);
```

to:

```rust
    // --- Repo row ---
    let is_repo = matches!(focus, NewAgentFocus::Repo);
    let repo_line = picker_row("Repo", repo_name, is_repo);
    frame.render_widget(Paragraph::new(repo_line), chunks[3]);
```

When `repos.len() <= 1` cycling is a no-op — that's fine, the footer hint advertises `←/→ cycle` only when relevant focus has more than one option. The visual chrome no longer changes based on that count, which matches "rules > exceptions."

- [ ] **Step 12.4: Replace the BranchToggle row**

Change `src/ui.rs:440-453` from:

```rust
    // --- Branch toggle row ---
    let is_toggle = matches!(focus, NewAgentFocus::BranchToggle);
    let mode_label = match branch_mode {
        BranchMode::New => "New",
        BranchMode::Existing => "Existing",
    };
    let toggle_line = Line::from(vec![
        Span::styled("  Branch", label_style(is_toggle)),
        Span::raw(" ".repeat((label_w as usize).saturating_sub(8))),
        Span::styled("\u{2039} ", Style::default().fg(if is_toggle { FOCUS } else { DIM })),
        Span::styled(mode_label, val_style(is_toggle)),
        Span::styled(" \u{203a}", Style::default().fg(if is_toggle { FOCUS } else { DIM })),
    ]);
    frame.render_widget(Paragraph::new(toggle_line), chunks[5]);
```

to:

```rust
    // --- Branch toggle row ---
    let is_toggle = matches!(focus, NewAgentFocus::BranchToggle);
    let mode_label = match branch_mode {
        BranchMode::New => "New",
        BranchMode::Existing => "Existing",
    };
    let toggle_line = picker_row("Branch", mode_label, is_toggle);
    frame.render_widget(Paragraph::new(toggle_line), chunks[5]);
```

- [ ] **Step 12.5: Confirm the Name row and BranchList still match the metaphor**

The Name row (`ui.rs:499-519`) and BranchList (`ui.rs:455-497`) already use the left-bar / inline pattern — no change needed, but read both to confirm: the BranchList uses `"\u{2502} "` for selected rows already (`ui.rs:481`), so the metaphor is consistent across the modal after this task.

The Name row currently shows the typed value inline without a left-bar indicator. That's acceptable because Name is a *typing* affordance, not a *picking* affordance — its focus signal is the cursor `_` glyph (`ui.rs:510`). Don't change it.

- [ ] **Step 12.6: Build and test**

Run: `cargo build && cargo test`
Expected: PASS.

- [ ] **Step 12.7: Manual verification**

Run: `cargo run`, press `n` to open the modal:
- Agent, Repo, Branch (toggle) rows show `│ Agent  <name>` / `│ Repo  <name>` / `│ Branch  <mode>` when focused; the bar disappears (replaced by two spaces) when focus moves away.
- No `‹ ›` arrows anywhere.
- Tab cycles focus through Agent → Repo → Branch (toggle) → BranchList → (Name) → Prompt.
- ←/→ still cycles values inside the focused picker row.
- The footer hint already advertises `←/→ cycle` (Task 5).

- [ ] **Step 12.8: Commit**

```bash
git add src/ui.rs
git commit -m "Unify modal selection metaphor on left-bar + ACCENT (drop arrow chrome)"
```

---

## Verification Pass

- [ ] **Final sweep**

Run: `cargo test && cargo build --release`
Expected: PASS, clean build.

Run: `grep -n "Color::" src/ui.rs`
Expected: no matches outside the import line. All color usage in `ui.rs` should reference `style::*` tokens.

Run: `grep -n "FOCUS\|READY\|\u{2039}\|\u{203a}" src/ui.rs`
Expected: no matches. Old token names and arrow glyphs are gone.

Run: `cargo run` and walk every surface:
1. Main view — footer reads `↑/k up · ↓/j down · n new · a attach · x stop · d delete · q quit`, keys bold, separator ` · `.
2. Empty state — body is plain dim copy; footer still advertises `n new`.
3. Agent table — selected row has `│` + cyan; spinner yellow when working, green when idle, red on error.
4. Drift arrow (slug→branch) — dim everywhere it appears.
5. Separator dot paginator — dots colored by status (green/yellow/red), current dot is filled.
6. New Agent modal — title bold (not cyan), borders dim, picker rows use `│` when focused, footer hint changes per focus and uses ` · ` separator with bold keys, j/k work in BranchList, q closes the modal from picker focuses but types literally in Prompt/Name.
7. Delete modal — title bold, hint reads `y delete · q/esc cancel` (or with-tmux variant), no "confirm" anywhere.

If any surface diverges, that's a regression — open the diff and trace it back to the relevant task.

- [ ] **Final commit (only if anything was missed)**

If the verification sweep finds and fixes any leftover divergence, commit it under a clear message. Otherwise, this plan ends with Task 12's commit.

---

## Out of Scope (intentionally not addressed)

These appeared in the audit but are deferred:

- **Two-line title/subtitle row pattern for the agent table.** z's table is column-shaped (BRANCH | BASE | REPO), not row-shaped (title / subtitle). Converting it is a bigger UX redesign that should happen in its own brainstorm, not as part of token rollout.
- **Faded selection state (selected-but-unfocused).** Today selected list rows drop from ACCENT to TEXT when the list loses focus (`ui.rs:482-487`). The audit flagged the lack of a third "faded" tier; that's a refinement, not a violation.
- **WARN color for user-facing error status messages.** Currently `app.status_message` renders dim regardless of severity (`ui.rs:334`). Adding a WARN/ALERT token would let errors stand out, but it requires plumbing severity through `App::status_message`, which expands scope. Track separately.
- **Border audit beyond modal titles.** Borders on modals are functional region boundaries — kept. No other places use `Borders::ALL` in `ui.rs`.
- **Placeholder distinction beyond italic+dim.** The pristine-name italic-dim signal at `ui.rs:502-508` is functioning. Adding a suffix `…` or background tint is not a violation, just a possible enhancement.

---

## Anti-pattern guardrails

While executing this plan, do not:

- Add a theming framework (no `Theme` struct, no runtime config, no `with_theme(...)` builders). Six constants is the whole system.
- Generalize `footer_hint` past `&[(&str, &str)]`. No alignment params, no separator override, no styling override. The shape we have today is the contract.
- Introduce snapshot tests / `insta` / `expect-test`. Plain unit tests on helpers + manual visual verification is the testing strategy.
- Centralize all `Style::default().fg(...)` calls into helper fns. Local `Style::default().fg(DIM)` is fine and readable; only extract a helper when a real second caller appears.
- Replace `q` in Prompt/Name focuses with cancel — those are text-input contexts; users type `q`. Tests in Task 8 enforce this.
- Touch `agent.rs`'s status enum semantics. `status_color` reads it; it is not modified.
