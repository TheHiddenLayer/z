# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.1](https://github.com/TheHiddenLayer/z/compare/v0.2.0...v0.2.1) - 2026-05-20

### Other

- add readme
- checkout before publishing releases
- tag merged release PRs
- wait for release PR checks

## [0.2.0](https://github.com/TheHiddenLayer/z/releases/tag/v0.2.0) - 2026-05-20

### Added

- *(tui)* reorder new-agent sources
- *(app)* order sessions by status
- upgrade ratatui wizard dividers
- *(wizard)* hint bar lives in global status row, not wizard panel
- *(wizard)* three group dividers separating sections
- *(wizard)* focus accent bar via Borders::LEFT
- *(ui)* polish new task wizard
- idiomatic ratatui scaffolding
- show GitLab MR state
- execute GitLab MR actions
- add MR workflow keys
- track MR state in app
- build glab MR commands
- add GitLab MR parsing

### Fixed

- *(panel)* clarify branch picker state
- *(tui)* coalesce stale ticks
- clear preview when session stops
- *(tui)* split quit and cancel keys
- *(app)* keep agent selection stable
- *(ui)* gate keymap legend globally
- *(ui)* unify wizard value styling
- *(ui)* remove agent table scrollbar
- tighten wizard alignment and dedupe selection cursors
- *(wizard)* make MR picker scroll
- drop alphabetical ordering from branches and agent table
- *(ui)* show wizard in preview panel
- stage tmux input via paste buffer
- harden MR worktree workflow
- render MR status as plain labels
- label MR status column
- harden MR confirmation and retry
- refine MR UI hints
- harden glab execution errors
- harden MR app state
- classify blocked GitLab MRs
- *(agent)* run check-ref-format inside repo, surface git stderr

### Other

- skip release-plz without token
- automate release publishing
- run workflow only on main
- avoid duplicate branch runs
- make CI tests portable
- add GitHub Actions workflow
- harden TUI architecture
- extract TUI architecture helpers
- Use editor for wizard prompt
- *(wizard)* final-review fixes (hint bar, list constraint, clippy)
- *(wizard)* remove dead layout helpers post-redesign
- *(wizard)* tighten name truncation, hint align, focus-frame helper
- *(wizard)* unify list payload + selected lookup across sources
- *(wizard)* switch list rendering to stock List + HighlightSpacing
- *(wizard)* render branch-mode list status into value sub-rect
- *(wizard)* render prompt body into value sub-rect
- *(wizard)* label gutter is temporary; Task 8 replaces with accent bar
- *(wizard)* split each row into label/value sub-rects
- *(wizard)* pin prompt body to 3-row rect
- *(wizard)* lock prompt body to Length(3) with trailing slack
- *(wizard)* update assertion for fixed 3-row prompt body
- *(wizard)* fixed vertical layout via Flex::Start
- *(wizard)* compare label column for stability check
- *(wizard)* pin focus-stability invariant
- *(plans)* wizard layout redesign implementation plan
- *(specs)* wizard layout redesign spec
- use ratatui stateful widgets
- Add GitLab task wizard
- cover GitLab MR workflow
- plan agentic GitLab MR flow
- design agentic GitLab MR flow
- Position dots: softer size step between active and passive
- Wizard rows: whole-row brightness for focus, match agent table
- Drop stale ACCENT reference from modal_title doc
- Remove ACCENT: monochrome chrome, status-only color
- Delete modal: bold TEXT for agent name, no cyan
- TEXT body always, _ cursor as focus signal
- Branch list: two-way TEXT/DIM, drop focus distinction
- Picker rows: TEXT brightness for focus, no cyan
- New Agent modal field styles: brightness on label, content-only on value
- Separator label: TEXT brightness, no cyan
- Agent table selection: brightness contrast, no cyan
- monochrome palette implementation
- monochrome palette — drop ACCENT, encode selection by brightness
- Hide footer keymap behind '?' toggle
- Swap separator alignment: dots left, title right
- Right-align session dot strip in the separator
- Unify modal selection metaphor on left-bar + ACCENT (drop arrow chrome)
- Modal titles use bold text; ACCENT is reserved for selection
- Drift arrow is a structural signal, not a status — render dim
- Move agent-creation hint from empty-state body to footer
- Cover Name focus in q-cancel guard tests
- Add q as cancel alias in modals (where it doesn't collide with text input)
- Tighten Task 7 tests: direct focus construction over walks
- Bind j/k in New Agent branch list
- Render delete modal hints via footer_hint and replace 'confirm' with 'delete'
- Render New Agent modal hints via footer_hint helper
- Render main status bar via footer_hint helper
- Add footer_hint helper
- Move status_color to style module
- Extract style tokens into dedicated module
- Add TUI style system plan
- Tighten TUI loop for snappier input
- Color session dots by agent status
- Replace count chip with dot paginator on session divider
- Strategic color palette: one meaning per color, terminal-themed
- Initial commit
