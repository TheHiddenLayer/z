# SCM MR inbox: simple provider boundary with GitLab v1

## Problem

`z` is already good at the local execution loop: configured repositories,
worktree-per-agent isolation, tmux sessions, attach/resume, and activity
detection. The missing layer is remote work visibility. Open merge requests,
CI failures, review state, and branch readiness live in GitLab, while the TUI
only knows about local agents.

The goal is to strengthen the development flywheel without turning `z` into a
scheduler. The first version should show all open MR work for configured repos
and make it cheap to manually launch or resume an agent on one MR branch.

Simplicity is the primary constraint.

## Decision

Add a source-control boundary in `src/scm.rs`. The app talks to a small Rust
trait, not directly to GitLab. GitLab is the only backend implemented in v1.

This gives us the useful software boundary now without pretending we need a
provider framework. The trait keeps GitLab-specific details out of `app.rs` and
`ui.rs`, while the concrete implementation can remain pragmatic and shell out
to `glab` for authentication and host configuration.

## Design principles

- Small trait, explicit data types, no provider registry.
- GitLab is the only implemented backend in v1.
- Prefer `glab` for v1 auth and host handling.
- Keep remote state read-only.
- Manual launch only; no automatic scheduler.
- Join remote MRs to local agents by repository plus source branch.
- Reuse the existing worktree/session machinery instead of introducing a
  parallel launch path.

## Core interface

`src/scm.rs` owns shared SCM types and the provider trait.

```rust
use std::path::PathBuf;
use futures::future::BoxFuture;

pub type ScmResult<T> = Result<T, ScmError>;

pub trait ScmProvider: Send + Sync {
    fn list_open_merge_requests<'a>(
        &'a self,
        repo: &'a ScmRepo,
    ) -> BoxFuture<'a, ScmResult<Vec<MergeRequest>>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScmRepo {
    pub local_path: PathBuf,
    pub name: String,
    pub remote_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeRequest {
    pub repo_name: String,
    pub iid: u64,
    pub title: String,
    pub source_branch: String,
    pub target_branch: String,
    pub web_url: String,
    pub state: MergeRequestState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeRequestState {
    Draft,
    CiFailed,
    Review,
    Ready,
    Unknown,
}
```

`ScmError` should be a small enum with enough detail for status messages and
tests:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScmError {
    RemoteUrlMissing { repo_name: String },
    CommandFailed { command: String, stderr: String },
    ParseFailed(String),
}
```

Use `BoxFuture` instead of adding `async_trait`. The crate already depends on
`futures`, and this keeps the interface explicit.

## GitLab backend

`GitLabScm` implements `ScmProvider`.

For v1 it shells out to `glab` from the repository directory:

```bash
glab mr list --state opened --output json
```

If extra fields are needed, add the smallest supported `glab` flags for those
fields. Do not introduce a native GitLab HTTP client in v1.

The backend maps GitLab data into the shared `MergeRequest` type. GitLab-only
details are discarded unless the UI needs them now.

State mapping:

| GitLab signal | `MergeRequestState` |
|---|---|
| draft/WIP MR | `Draft` |
| failed pipeline | `CiFailed` |
| unresolved discussions or review requested, when present in the chosen `glab` JSON shape | `Review` |
| open, non-draft, successful pipeline | `Ready` |
| missing or ambiguous fields | `Unknown` |

The implementation must first inspect the JSON shape returned by the selected
`glab` command. If review/discussion fields are absent, do not add another API
call in v1. Map those MRs by the remaining signals and use `Unknown` when the
state cannot be inferred.

## App integration

Add MR refresh actions and commands alongside the existing agent refresh flow:

```rust
Action::ToggleView
Action::RefreshMergeRequests
Action::MergeRequestsRefreshed(Vec<MergeRequest>)
Action::MergeRequestsFailed(String)
Action::LaunchSelectedMergeRequest

Command::RefreshMergeRequests(Vec<PathBuf>)
```

`App` stores:

```rust
pub merge_requests: Vec<MergeRequest>,
pub selected_mr: usize,
pub view: View,
```

where:

```rust
pub enum View {
    Agents,
    MergeRequests,
}
```

The MR refresh command resolves configured repo paths into `ScmRepo` values,
calls the GitLab provider for each repo, and sends one combined result back to
the state machine.

## Local correlation

The UI derives local MR status by joining:

- `MergeRequest.repo_name`
- `MergeRequest.source_branch`

against existing `Agent` rows:

- `Agent.repo_name`
- `Agent.branch`

If a match exists, the MR row can show the existing agent status:

- running spinner/check/error from the agent
- stopped when the worktree exists but no tmux session exists

This is derived display state only. Do not duplicate agent lifecycle state in
the MR model.

## Manual launch

In the MR view, selecting an MR and pressing the launch key should:

1. If a matching local agent exists and has a session, attach to it.
2. If a matching local agent exists but is stopped, resume it.
3. If no local agent exists, create an agent using the existing-branch path for
   the MR source branch.

This should reuse the existing `CreateAgent`, `PrepareAttach`, and `Attach`
machinery as much as possible. The MR feature should not introduce a second
worktree/session creation model.

## UI

Add a second top-level view rather than a large dashboard.

Normal mode keys:

- `m` toggles between Agents and Merge Requests.
- Existing agent keys keep their current behavior in the Agents view.
- In the MR view, `j/k` or arrows move selection.
- In the MR view, `a` or Enter launches/resumes/attaches the selected MR.
- In the MR view, `r` refreshes MRs.

MRs load when the view opens and when the user presses `r`. Do not poll GitLab
on the existing high-frequency tick cadence in v1.

MR rows should stay compact:

```text
STATE  !IID  SOURCE -> TARGET  TITLE  REPO
```

Use the existing monochrome design rules. Status color remains reserved for
agent status. MR state labels should be text-first and subdued unless they
reuse existing status semantics clearly.

## Configuration

No new provider configuration in v1.

Configured `repos` remain the source of truth. The GitLab backend infers GitLab
context from each repo's `origin` remote and the user's existing `glab`
authentication.

If `glab` is missing or unauthenticated, show a status message and keep the app
usable.

## Testing

Unit tests in `src/scm.rs`:

- parse `glab mr list --output json` into `MergeRequest`
- map draft MRs to `Draft`
- map failed pipelines to `CiFailed`
- map successful non-draft MRs to `Ready`
- preserve `Unknown` when optional fields are absent
- return `ParseFailed` on malformed JSON

App tests:

- MR refresh stores returned MRs
- MR selection navigation is independent of agent selection
- MR launch attaches when a matching running agent exists
- MR launch resumes when a matching stopped agent exists
- MR launch creates an existing-branch agent when no local agent exists

No snapshot tests are required.

## Out of scope

- Automatic agent scheduling.
- Provider registry or config-driven provider selection.
- GitHub, Bitbucket, or generic forge implementations.
- Native GitLab API client.
- Posting comments, resolving threads, retrying CI, rebasing, merging, or
  mutating MRs remotely.
- Persisting MR cache to disk.
- Assigning agents automatically based on CI or review state.

## Success criteria

- `z` can show open GitLab MRs for every configured repo.
- The app remains usable when GitLab or `glab` fails.
- A selected MR can manually launch, resume, or attach to an agent.
- GitLab-specific parsing and command execution stay behind `ScmProvider`.
- The first implementation stays small enough that later automation can be
  added from evidence rather than speculation.
