# GitLab New Task Wizard

## Problem

z already has a compact New Agent modal for creating a worktree, starting a
tmux session, and launching a configured agent. It is fast, but it assumes the
user already knows the branch and prompt. Emdash has a stronger New Task flow:
the user can start from a branch, issue, or pull request, and the source context
drives task naming, branch strategy, and initial conversation.

z should gain the same low-friction source-based workflow without becoming a
large provider platform. The target is a calm progressive wizard for GitLab
only, using the authenticated `glab` CLI.

## Decisions

- Use the existing New Agent modal as the integration point instead of adding a
  second creation surface.
- Use a progressive flow: pick the source first (`issue`, `mr`, `branch`), then
  reveal only the controls needed for that source.
- Fetch GitLab data through `glab`, not direct API tokens in z config.
- v1 supports GitLab only.
- Issue lists default to open issues assigned to the current user.
- MR lists default to open merge requests where the current user is a reviewer.
- Issue-created tasks create a new z branch using the GitLab issue number.
- MR-created tasks review the MR head branch directly.
- Every source gets a generated prompt by default, with an explicit escape hatch
  to edit the prompt before starting.

## UX Flow

Opening the wizard lands on a focused source row:

```text
| Start from  issue
  Agent       codex
  Repo        example
```

`Start from` cycles between `issue`, `mr`, and `branch`. Agent and repo stay
visible, but the source choice is the primary action.

### Issue Source

When `issue` is selected, z fetches assigned open GitLab issues for the selected
repo. The picker shows a compact list with issue number and title. Search
filters the loaded list locally.

Selecting an issue prepares:

- branch: `z-<MMDD>-<issue-number>-<slug-title>`
- base branch: selected branch, defaulting to `main` / `master` when present
- prompt: generated issue prompt

If the generated branch already exists, z appends `-2`, `-3`, and so on.

### MR Source

When `mr` is selected, z fetches open GitLab merge requests where the current
user is a reviewer. The picker shows MR number, title, source branch, and target
branch when available. Search filters locally.

Selecting an MR prepares:

- branch: MR source branch / head branch
- branch mode: existing branch / direct checkout
- prompt: generated review prompt

MR flow does not create a separate z branch in v1. It creates or attaches a
worktree on the MR head branch so the agent reviews exactly what the MR contains.
If the branch is not already available locally, z prepares it without switching
the main worktree by fetching the GitLab MR head/source ref into the local repo
before creating the worktree.

### Branch Source

The `branch` source preserves the current branch-based flow:

- choose new or existing branch
- choose base branch for new branches
- edit generated branch name
- optionally edit prompt

This source remains the escape hatch for ad hoc work that is not tied to GitLab.

### Prompt Escape Hatch

The prompt begins in generated mode. Source changes regenerate the prompt only
while it is still generated. Pressing `e` enters prompt editing. Typing or
backspacing in the prompt marks it custom; merely tabbing through the prompt
field does not. After that, source changes preserve the custom prompt unless the
user explicitly resets it with `r` while the prompt field is focused.

The start action sends the prompt to the agent exactly as shown. An empty custom
prompt is still allowed and preserves the current behavior: z creates the
session and the user can attach and type manually.

## Prompt Templates

Issue prompt:

```text
Work on GitLab issue #<number>: <title>
<web_url>

<description>
```

MR prompt:

```text
Review GitLab MR !<number>: <title>
<source_branch> -> <target_branch>
<web_url>

<description>
```

Descriptions are omitted if GitLab returns none. The templates stay plain text
so they work with every configured agent command.

## Architecture

Add a focused `src/gitlab.rs` module that owns `glab` interaction and JSON
parsing. It exposes small structs:

- `GitlabIssue`
- `GitlabMergeRequest`
- `GitlabError`

The app state machine owns wizard state:

- selected source
- source focus
- loading / loaded / failed status for issue and MR lists
- local search text
- selected issue or MR
- branch name
- prompt text and generated/custom status

`main.rs` executes new async commands:

- `LoadGitlabIssues { repo }`
- `LoadGitlabMrs { repo }`
- optional detail fetch commands if list JSON lacks description fields
- `PrepareGitlabMrBranch { repo, mr_iid, source_branch }` if the selected MR
  head branch is not already usable as a local worktree branch

After the wizard resolves the creation intent, it reuses the existing agent
creation command shape:

```rust
Command::CreateAgent {
    repo,
    branch,
    new_branch,
    base_branch,
    session_name,
    agent_name,
    fresh_cmd,
}
```

Issue and branch sources use `new_branch = true` when creating a new z branch.
MR source uses `new_branch = false` with the MR head branch after the branch has
been prepared locally.

## glab Commands

Issue list:

```bash
glab issue list --assignee=@me --output json --per-page 30
```

MR list:

```bash
glab mr list --reviewer=@me --output json --per-page 30
```

Detail commands are used only if needed for prompt descriptions:

```bash
glab issue view <iid> --output json
glab mr view <iid-or-branch> --output json
```

MR branch preparation should not use `glab mr checkout` because that switches
the main worktree. It should use Git plumbing from the selected repo, preferring
the source branch if it exists locally and otherwise fetching the MR head/source
ref into a local branch before running `git worktree add`.

All commands run with the selected repo as their current directory so `glab`
uses that repository context.

## Error Handling

Errors render inline in the picker area and never close the wizard:

- `glab` missing
- `glab` unauthenticated
- selected repo has no GitLab remote / `glab` cannot resolve repo context
- network or GitLab API failure
- no assigned issues
- no MRs needing review
- malformed JSON from `glab`

The footer shows retry and navigation hints. The user can switch to another
source, another repo, or branch mode without losing the wizard.

## Rendering Model

Keep the existing monochrome TUI rules:

- selection and focus use `TEXT` vs `DIM` plus the `│` indicator
- status colors remain reserved for agent status glyphs
- loading/error/empty rows are dim structural text, not new accent colors

The modal title can remain `New Agent`, but source rows should make the task
origin clear. A future rename to `New Task` is optional; it is not required for
v1 because z still creates an agent session as the concrete artifact.

## Tests

Add focused unit coverage for:

- `glab` issue JSON parsing
- `glab` MR JSON parsing
- missing / malformed JSON errors
- issue branch-name generation, including collision suffixes
- issue prompt generation
- MR prompt generation
- generated prompt reset vs custom prompt preservation
- source switching state transitions
- issue fetch result handling
- MR fetch result handling
- MR selection emitting `CreateAgent` with `new_branch = false`
- issue selection emitting `CreateAgent` with issue-number branch naming
- existing branch flow regression coverage
- key handling for source picker, source list, search, and prompt edit states

## Out Of Scope

- GitHub, Jira, Linear, Forgejo, or direct GitLab API token support.
- Persistent issue/MR cache.
- Full-text remote search on every keystroke. v1 fetches a bounded list and
  filters locally.
- Creating a separate review branch for MRs.
- Editing GitLab issues or MRs from z.
- Posting comments or review feedback back to GitLab.
- Configuring GitLab credentials in z.
