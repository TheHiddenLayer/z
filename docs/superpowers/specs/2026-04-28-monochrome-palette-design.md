# Monochrome palette: drop ACCENT, encode selection by brightness

## Problem

z's TUI uses six semantic color tokens today (`src/style.rs:12-17`):
`TEXT`, `DIM`, `ACCENT` (cyan), `OK` (green), `BUSY` (yellow), `FAIL` (red).
Recent work has been narrowing color's role — drift arrow moved to DIM, modal
titles moved to bold-TEXT — but `ACCENT` is still doing redundant work.
Selection in the agent table is encoded *three* ways at once: a `│` left-bar
glyph, ACCENT cyan on the bar, and ACCENT cyan on the row's text (with DIM on
unselected rows). The bar plus brightness contrast already make selection
unambiguous; the cyan layer is decoration.

The goal is a calmer, more monochrome interface. Color should mean *one* thing:
agent status. Selection and focus should be encoded structurally and by
brightness, not by hue.

## Decision

Remove `ACCENT` from the palette. Replace every callsite with a monochrome
encoding (brightness contrast, structural glyphs, weight). Status colors
(`OK`/`BUSY`/`FAIL`) stay exactly as they are.

### Final tokens (`src/style.rs`)

- `TEXT` (Reset) — kept
- `DIM` (DarkGray) — kept
- `OK` (Green) — kept, unchanged shade
- `BUSY` (Yellow) — kept, unchanged shade
- `FAIL` (Red) — kept, unchanged shade
- `ACCENT` — **removed**

Status colors deliberately stay as ratatui's named ANSI colors so they inherit
from the user's terminal theme. Hardcoding RGB / 256-color shades (e.g., a
specific orange for BUSY) would override theme harmony. No current pain point
justifies that override.

### Selection and focus encoding

| Surface | Today | New |
|---|---|---|
| Agent table selected row (`ui.rs:140-178`) | `│` cyan + cyan text; unselected = DIM | `│` `TEXT` + `TEXT` text; unselected = DIM |
| Separator branch label (`ui.rs:210-225`) | cyan | `TEXT` (pops vs surrounding DIM chrome) |
| Modal field label, focus state (`ui.rs:377`) | cyan when focused; DIM otherwise | `TEXT` when focused; DIM otherwise |
| Modal field value, focus state (`ui.rs:380`) | cyan when focused; TEXT otherwise | `TEXT` regardless — value is content, focus lives on the indicator + label |
| Picker row indicator (`ui.rs:387-392`) | `│` cyan when focused; space otherwise | `│` `TEXT` when focused; space otherwise |
| Picker row label (`ui.rs:393-397`) | cyan focused; DIM unfocused | `TEXT` focused; DIM unfocused |
| Picker row value (`ui.rs:398-402`) | cyan focused; TEXT unfocused | `TEXT` regardless |
| Branch list selection in New Agent modal (`ui.rs:461-465`) | cyan | `│` + `TEXT`; unselected = DIM |
| Prompt textarea body (`ui.rs:528`) | cyan when `is_prompt`, TEXT otherwise | `TEXT` always — body color does not encode mode |
| Prompt synthetic `_` cursor (`ui.rs:510`) | cyan | `TEXT` (its presence at the caret is already the focus signal) |
| Delete modal — inline agent name (`ui.rs:590`) | cyan | bold `TEXT` (inline noun emphasis) |

### Status glyphs — unchanged

The agent-table status glyph (`ui.rs:77-92`) and the separator dot strip
(`ui.rs:236-238`) both keep `status_color()`. These are the *only* surfaces
that carry hue. The agent table also retains shape redundancy (`✓`/spinner/
`✗`/`−`); the dot strip is color-only by design.

### Module doc

Rewrite the header in `src/style.rs:1-6`:

> Five semantic colors. Status colors (OK/BUSY/FAIL) appear only on agent
> status indicators — never in chrome. Selection and focus are encoded
> monochromatically: brightness contrast (`TEXT` vs `DIM`) plus structural
> glyphs like the `│` left bar. Color is reserved for status meaning.

## Rationale

**Why brightness contrast for selection.** The `│` left bar is already a
structural, unambiguous selection cue. Layering cyan on top is decoration.
Brightness (TEXT vs DIM) is the obvious secondary signal: the eye reads
"bright row in a sea of dim rows" at least as fast as "cyan row in a sea of
gray rows," without introducing a hue that competes with status colors.

**Why bold for the delete-modal name.** The agent name in `Delete <name>?`
needs to pop as a *noun* inside a sentence — not as "selected." Bold weight is
the right tool for inline noun emphasis. It's distinct from the modal-title
bold (which is a title context, not an inline word) and from footer-hint bold
(which marks key glyphs adjacent to verb labels). Each bold use reads
unambiguously from its surroundings.

**Why the prompt textarea body shouldn't change color by mode.** Coloring
prose by application mode is a category error: body text should be readable as
text, not as a state indicator. Mode belongs on chrome — the surrounding
modal/pane border (already getting the focused-vs-unfocused brightness shift)
plus the synthetic `_` cursor (only rendered in prompt mode) plus the footer
hints are three independent focus signals. Removing the prose recolor is a
correctness improvement, not just a palette change.

**Why status colors stay named.** `Color::Green` / `Color::Yellow` /
`Color::Red` map to ANSI 1-3 and inherit the user's terminal theme. A user who
has dialed in a Solarized or Gruvbox palette gets z that looks native to their
shell. Replacing these with hardcoded shades to chase a specific aesthetic in
one terminal would break that contract for everyone else.

## Tests

Required updates in `src/style.rs`:

- Remove `ACCENT` constant + `tokens_have_expected_colors`'s `ACCENT`
  assertion.
- Rewrite `modal_title_is_bold_text_not_accent` and `drift_arrow_is_dim_not_busy`
  to assert positive expected styles instead of `assert_ne!` against the
  removed `ACCENT`.

New regression tests in the appropriate modules:

- Agent-table selected indicator span has fg = `TEXT` (regression guard so
  cyan can't creep back in via copy-paste).
- Prompt textarea body span has fg = `TEXT` regardless of `is_prompt`.

Existing tests for footer_hint, status_color, and selection navigation should
continue to pass unchanged.

## Out of scope

- **Retuning the status-color shades** (e.g., orange for BUSY, softer red for
  FAIL). The 3-status structure stays; the shades stay named ANSI. Revisit
  only with concrete contrast complaints.
- **Reverse-video / background-color selection styles.** Considered and
  rejected as too loud against the calm-monochrome goal.
- **Bold on selected row text.** Reserved as a future lever; not introduced
  now to avoid colliding with the modal-title and footer-hint bold
  conventions.
- **Animations / blinking cursors.** The synthetic `_` is a static glyph; no
  `Modifier::SLOW_BLINK`.
- **Touching `status_glyph` shapes or `status_color()` mappings.** Status
  rendering is unchanged.

## Anti-patterns to avoid

- Reintroducing a "highlight color" later under a different name. If a future
  feature thinks it needs a fourth hue, the answer is bold or a structural
  glyph, not a new constant.
- Using bold for "selected" in lists. Bold is for inline noun emphasis and
  footer keys; selection is `│` + brightness.
- Coloring prose by mode. Body text is `TEXT`; mode lives on chrome.
- Hardcoding 256-color shades for status. Stay on named ANSI so terminal
  themes win.
