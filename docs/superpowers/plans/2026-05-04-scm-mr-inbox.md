# SCM MR Inbox Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a simple GitLab-backed MR inbox to `z`, behind a small `src/scm.rs` trait boundary, with manual agent launch/resume/attach from a selected MR.

**Architecture:** Introduce `src/scm.rs` as the only SCM boundary: shared types, `ScmProvider`, GitLab parsing, GitLab command execution, and repo resolution helpers live there. `App` stores MR rows and a top-level `View`, while `main.rs` executes the GitLab refresh command asynchronously. `ui.rs` renders either the existing agent table or the new MR table; MR launch reuses the existing agent lifecycle commands.

**Tech Stack:** Rust 2024, tokio, futures `BoxFuture`, serde, serde_json, ratatui, crossterm, `glab` CLI.

---

## File Structure

| File | Status | Responsibility |
|---|---|---|
| `Cargo.toml` | Modify | Add `serde_json` for `glab --output json` parsing. |
| `src/main.rs` | Modify | Add `mod scm;` and execute `Command::RefreshMergeRequests`. |
| `src/scm.rs` | Create | SCM trait, shared MR types, GitLab parser, GitLab provider, repo resolution helpers. |
| `src/app.rs` | Modify | Add MR state, top-level view mode, MR refresh actions, MR selection, manual launch. |
| `src/ui.rs` | Modify | Render agent or MR table depending on view; add MR-specific status bar, separator, and preview content. |

## Task 1: Create `src/scm.rs` with pure MR types and GitLab JSON parsing

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/main.rs`
- Create: `src/scm.rs`

- [ ] **Step 1: Write the failing parser tests**

Add the module declaration near the existing module list in `src/main.rs`:

```rust
mod config;
mod agent;
mod app;
mod notifications;
mod scm;
mod style;
mod ui;
```

Create `src/scm.rs` with these tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_JSON: &str = r#"[
      {
        "iid": 1,
        "title": "Draft: add queue",
        "source_branch": "feature/queue",
        "target_branch": "main",
        "web_url": "https://gitlab.example.com/acme/app/-/merge_requests/1",
        "draft": true,
        "pipeline": { "status": "success" }
      },
      {
        "iid": 2,
        "title": "Fix flaky test",
        "source_branch": "fix/flaky",
        "target_branch": "main",
        "web_url": "https://gitlab.example.com/acme/app/-/merge_requests/2",
        "pipeline": { "status": "failed" }
      },
      {
        "iid": 3,
        "title": "Ready change",
        "source_branch": "ready/change",
        "target_branch": "main",
        "web_url": "https://gitlab.example.com/acme/app/-/merge_requests/3",
        "pipeline": { "status": "success" }
      },
      {
        "iid": 4,
        "title": "Needs review",
        "source_branch": "review/change",
        "target_branch": "main",
        "web_url": "https://gitlab.example.com/acme/app/-/merge_requests/4",
        "blocking_discussions_resolved": false
      },
      {
        "iid": 5,
        "title": "Unknown change",
        "source_branch": "unknown/change",
        "target_branch": "main",
        "web_url": "https://gitlab.example.com/acme/app/-/merge_requests/5"
      }
    ]"#;

    #[test]
    fn parses_gitlab_merge_requests() {
        let mrs = parse_gitlab_merge_requests("app", SAMPLE_JSON).unwrap();
        assert_eq!(mrs.len(), 5);
        assert_eq!(mrs[0].repo_name, "app");
        assert_eq!(mrs[0].iid, 1);
        assert_eq!(mrs[0].title, "Draft: add queue");
        assert_eq!(mrs[0].source_branch, "feature/queue");
        assert_eq!(mrs[0].target_branch, "main");
        assert_eq!(
            mrs[0].web_url,
            "https://gitlab.example.com/acme/app/-/merge_requests/1"
        );
    }

    #[test]
    fn maps_merge_request_states() {
        let mrs = parse_gitlab_merge_requests("app", SAMPLE_JSON).unwrap();
        assert_eq!(mrs[0].state, MergeRequestState::Draft);
        assert_eq!(mrs[1].state, MergeRequestState::CiFailed);
        assert_eq!(mrs[2].state, MergeRequestState::Ready);
        assert_eq!(mrs[3].state, MergeRequestState::Review);
        assert_eq!(mrs[4].state, MergeRequestState::Unknown);
    }

    #[test]
    fn accepts_camel_case_glab_fields() {
        let json = r#"[{
          "iid": 7,
          "title": "Camel case",
          "sourceBranch": "camel/source",
          "targetBranch": "main",
          "webUrl": "https://gitlab.example.com/acme/app/-/merge_requests/7",
          "workInProgress": true
        }]"#;
        let mrs = parse_gitlab_merge_requests("app", json).unwrap();
        assert_eq!(mrs[0].source_branch, "camel/source");
        assert_eq!(mrs[0].target_branch, "main");
        assert_eq!(mrs[0].web_url, "https://gitlab.example.com/acme/app/-/merge_requests/7");
        assert_eq!(mrs[0].state, MergeRequestState::Draft);
    }

    #[test]
    fn malformed_json_returns_parse_failed() {
        let err = parse_gitlab_merge_requests("app", "not-json").unwrap_err();
        assert!(matches!(err, ScmError::ParseFailed(_)));
    }
}
```

- [ ] **Step 2: Run the parser test and verify it fails**

Run:

```bash
cargo test scm::tests::parses_gitlab_merge_requests
```

Expected: FAIL at compile time with missing `parse_gitlab_merge_requests`, `MergeRequestState`, or `ScmError`.

- [ ] **Step 3: Add `serde_json`**

In `Cargo.toml`, change the dependency section to include `serde_json`:

```toml
[dependencies]
ratatui = "0.29"
crossterm = { version = "0.28", features = ["event-stream"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "process", "sync", "fs"] }
futures = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = { version = "0.8", features = ["preserve_order"] }
dirs = "6"
tokio-util = "0.7"
notify-rust = "4"
```

- [ ] **Step 4: Implement the pure SCM types and parser**

Replace `src/scm.rs` with this implementation, preserving the tests from Step 1 at the bottom:

```rust
use futures::future::BoxFuture;
use serde::Deserialize;
use std::fmt;
use std::path::PathBuf;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScmError {
    RemoteUrlMissing { repo_name: String },
    CommandFailed { command: String, stderr: String },
    ParseFailed(String),
}

impl fmt::Display for ScmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScmError::RemoteUrlMissing { repo_name } => {
                write!(f, "repo '{repo_name}' has no origin remote")
            }
            ScmError::CommandFailed { command, stderr } => {
                write!(f, "{command} failed: {}", stderr.trim())
            }
            ScmError::ParseFailed(msg) => write!(f, "SCM parse failed: {msg}"),
        }
    }
}

impl std::error::Error for ScmError {}

#[derive(Debug, Deserialize)]
struct GitLabMergeRequest {
    iid: u64,
    title: String,
    #[serde(alias = "sourceBranch")]
    source_branch: String,
    #[serde(alias = "targetBranch")]
    target_branch: String,
    #[serde(alias = "webUrl")]
    web_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default, alias = "workInProgress")]
    work_in_progress: bool,
    #[serde(default)]
    pipeline: Option<GitLabPipeline>,
    #[serde(default, alias = "headPipeline")]
    head_pipeline: Option<GitLabPipeline>,
    #[serde(default, alias = "blockingDiscussionsResolved")]
    blocking_discussions_resolved: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct GitLabPipeline {
    #[serde(default)]
    status: String,
}

pub fn parse_gitlab_merge_requests(
    repo_name: &str,
    json: &str,
) -> ScmResult<Vec<MergeRequest>> {
    let raw: Vec<GitLabMergeRequest> =
        serde_json::from_str(json).map_err(|e| ScmError::ParseFailed(e.to_string()))?;
    Ok(raw
        .into_iter()
        .map(|mr| {
            let state = infer_gitlab_state(&mr);
            MergeRequest {
                repo_name: repo_name.to_string(),
                iid: mr.iid,
                title: mr.title,
                source_branch: mr.source_branch,
                target_branch: mr.target_branch,
                web_url: mr.web_url,
                state,
            }
        })
        .collect())
}

fn infer_gitlab_state(mr: &GitLabMergeRequest) -> MergeRequestState {
    if mr.draft
        || mr.work_in_progress
        || mr.title.starts_with("Draft:")
        || mr.title.starts_with("WIP:")
    {
        return MergeRequestState::Draft;
    }

    let pipeline_status = mr
        .pipeline
        .as_ref()
        .or(mr.head_pipeline.as_ref())
        .map(|p| p.status.as_str());

    if matches!(pipeline_status, Some("failed")) {
        return MergeRequestState::CiFailed;
    }

    if mr.blocking_discussions_resolved == Some(false) {
        return MergeRequestState::Review;
    }

    if matches!(pipeline_status, Some("success")) {
        return MergeRequestState::Ready;
    }

    MergeRequestState::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_JSON: &str = r#"[
      {
        "iid": 1,
        "title": "Draft: add queue",
        "source_branch": "feature/queue",
        "target_branch": "main",
        "web_url": "https://gitlab.example.com/acme/app/-/merge_requests/1",
        "draft": true,
        "pipeline": { "status": "success" }
      },
      {
        "iid": 2,
        "title": "Fix flaky test",
        "source_branch": "fix/flaky",
        "target_branch": "main",
        "web_url": "https://gitlab.example.com/acme/app/-/merge_requests/2",
        "pipeline": { "status": "failed" }
      },
      {
        "iid": 3,
        "title": "Ready change",
        "source_branch": "ready/change",
        "target_branch": "main",
        "web_url": "https://gitlab.example.com/acme/app/-/merge_requests/3",
        "pipeline": { "status": "success" }
      },
      {
        "iid": 4,
        "title": "Needs review",
        "source_branch": "review/change",
        "target_branch": "main",
        "web_url": "https://gitlab.example.com/acme/app/-/merge_requests/4",
        "blocking_discussions_resolved": false
      },
      {
        "iid": 5,
        "title": "Unknown change",
        "source_branch": "unknown/change",
        "target_branch": "main",
        "web_url": "https://gitlab.example.com/acme/app/-/merge_requests/5"
      }
    ]"#;

    #[test]
    fn parses_gitlab_merge_requests() {
        let mrs = parse_gitlab_merge_requests("app", SAMPLE_JSON).unwrap();
        assert_eq!(mrs.len(), 5);
        assert_eq!(mrs[0].repo_name, "app");
        assert_eq!(mrs[0].iid, 1);
        assert_eq!(mrs[0].title, "Draft: add queue");
        assert_eq!(mrs[0].source_branch, "feature/queue");
        assert_eq!(mrs[0].target_branch, "main");
        assert_eq!(
            mrs[0].web_url,
            "https://gitlab.example.com/acme/app/-/merge_requests/1"
        );
    }

    #[test]
    fn maps_merge_request_states() {
        let mrs = parse_gitlab_merge_requests("app", SAMPLE_JSON).unwrap();
        assert_eq!(mrs[0].state, MergeRequestState::Draft);
        assert_eq!(mrs[1].state, MergeRequestState::CiFailed);
        assert_eq!(mrs[2].state, MergeRequestState::Ready);
        assert_eq!(mrs[3].state, MergeRequestState::Review);
        assert_eq!(mrs[4].state, MergeRequestState::Unknown);
    }

    #[test]
    fn accepts_camel_case_glab_fields() {
        let json = r#"[{
          "iid": 7,
          "title": "Camel case",
          "sourceBranch": "camel/source",
          "targetBranch": "main",
          "webUrl": "https://gitlab.example.com/acme/app/-/merge_requests/7",
          "workInProgress": true
        }]"#;
        let mrs = parse_gitlab_merge_requests("app", json).unwrap();
        assert_eq!(mrs[0].source_branch, "camel/source");
        assert_eq!(mrs[0].target_branch, "main");
        assert_eq!(mrs[0].web_url, "https://gitlab.example.com/acme/app/-/merge_requests/7");
        assert_eq!(mrs[0].state, MergeRequestState::Draft);
    }

    #[test]
    fn malformed_json_returns_parse_failed() {
        let err = parse_gitlab_merge_requests("app", "not-json").unwrap_err();
        assert!(matches!(err, ScmError::ParseFailed(_)));
    }
}
```

- [ ] **Step 5: Run parser tests**

Run:

```bash
cargo test scm::tests
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs src/scm.rs
git commit -m "Add SCM merge request parser"
```

## Task 2: Implement the GitLab provider and repo resolution helpers

**Files:**
- Modify: `src/scm.rs`

- [ ] **Step 1: Add failing tests for repo naming and `glab` args**

Append these tests inside `src/scm.rs`'s existing `tests` module:

```rust
    #[test]
    fn repo_name_from_path_uses_directory_name() {
        let path = std::path::Path::new("/tmp/work/myapp");
        assert_eq!(repo_name_from_path(path).unwrap(), "myapp");
    }

    #[test]
    fn gitlab_mr_list_args_are_stable() {
        assert_eq!(
            GitLabScm::mr_list_args(),
            ["mr", "list", "--state", "opened", "--output", "json"]
        );
    }
```

- [ ] **Step 2: Run the new tests and verify they fail**

Run:

```bash
cargo test scm::tests
```

Expected: FAIL at compile time with missing `repo_name_from_path` and `GitLabScm`.

- [ ] **Step 3: Implement repo resolution and `GitLabScm`**

Add these imports near the top of `src/scm.rs`:

```rust
use futures::FutureExt;
use std::path::Path;
use tokio::process::Command;
```

Keep the existing `BoxFuture`, `Deserialize`, `fmt`, and `PathBuf` imports.

Add this implementation above the `#[cfg(test)]` module:

```rust
#[derive(Debug, Default, Clone, Copy)]
pub struct GitLabScm;

impl GitLabScm {
    pub fn mr_list_args() -> [&'static str; 6] {
        ["mr", "list", "--state", "opened", "--output", "json"]
    }
}

impl ScmProvider for GitLabScm {
    fn list_open_merge_requests<'a>(
        &'a self,
        repo: &'a ScmRepo,
    ) -> BoxFuture<'a, ScmResult<Vec<MergeRequest>>> {
        async move {
            let output = Command::new("glab")
                .args(Self::mr_list_args())
                .current_dir(&repo.local_path)
                .output()
                .await
                .map_err(|e| ScmError::CommandFailed {
                    command: "glab mr list".to_string(),
                    stderr: e.to_string(),
                })?;

            if !output.status.success() {
                return Err(ScmError::CommandFailed {
                    command: "glab mr list".to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }

            parse_gitlab_merge_requests(&repo.name, &String::from_utf8_lossy(&output.stdout))
        }
        .boxed()
    }
}

pub fn repo_name_from_path(path: &Path) -> ScmResult<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToString::to_string)
        .ok_or_else(|| ScmError::RemoteUrlMissing {
            repo_name: path.display().to_string(),
        })
}

pub async fn scm_repo_from_path(path: &Path) -> ScmResult<ScmRepo> {
    let name = repo_name_from_path(path)?;
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["remote", "get-url", "origin"])
        .output()
        .await
        .map_err(|e| ScmError::CommandFailed {
            command: "git remote get-url origin".to_string(),
            stderr: e.to_string(),
        })?;

    if !output.status.success() {
        return Err(ScmError::RemoteUrlMissing { repo_name: name });
    }

    let remote_url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if remote_url.is_empty() {
        return Err(ScmError::RemoteUrlMissing { repo_name: name });
    }

    Ok(ScmRepo {
        local_path: path.to_path_buf(),
        name,
        remote_url,
    })
}
```

- [ ] **Step 4: Run SCM tests**

Run:

```bash
cargo test scm::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/scm.rs
git commit -m "Implement GitLab SCM provider"
```

## Task 3: Add MR state, view mode, refresh actions, and MR navigation to `App`

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add failing app tests for MR refresh, view toggle, and MR selection**

Inside `src/app.rs`'s existing `tests` module, add this helper near `mock_agent`:

```rust
    fn mock_merge_request(repo_name: &str, source_branch: &str, iid: u64) -> crate::scm::MergeRequest {
        crate::scm::MergeRequest {
            repo_name: repo_name.into(),
            iid,
            title: format!("MR {iid}"),
            source_branch: source_branch.into(),
            target_branch: "main".into(),
            web_url: format!("https://gitlab.example.com/acme/{repo_name}/-/merge_requests/{iid}"),
            state: crate::scm::MergeRequestState::Ready,
        }
    }
```

Add these tests:

```rust
    #[test]
    fn toggle_view_switches_to_merge_requests_and_requests_refresh() {
        let mut app = test_app();
        let cmds = app.update(Action::ToggleView);
        assert_eq!(app.view, View::MergeRequests);
        assert!(matches!(cmds.as_slice(), [Command::RefreshMergeRequests(_)]));
    }

    #[test]
    fn merge_requests_refreshed_stores_rows_and_clamps_selection() {
        let mut app = test_app();
        app.view = View::MergeRequests;
        app.selected_mr = 9;
        app.update(Action::MergeRequestsRefreshed(vec![
            mock_merge_request("myapp", "feature/a", 1),
            mock_merge_request("myapp", "feature/b", 2),
        ]));
        assert_eq!(app.merge_requests.len(), 2);
        assert_eq!(app.selected_mr, 1);
    }

    #[test]
    fn mr_navigation_is_independent_of_agent_selection() {
        let mut app = test_app();
        app.view = View::MergeRequests;
        app.agents = vec![mock_agent("agent-a"), mock_agent("agent-b")];
        app.selected = 1;
        app.merge_requests = vec![
            mock_merge_request("myapp", "feature/a", 1),
            mock_merge_request("myapp", "feature/b", 2),
        ];

        app.update(Action::MoveDown);

        assert_eq!(app.selected, 1);
        assert_eq!(app.selected_mr, 1);
    }

    #[test]
    fn r_key_refreshes_merge_requests_only_in_mr_view() {
        let mut app = test_app();
        assert!(app.handle_key(make_key(KeyCode::Char('r'))).is_none());
        app.view = View::MergeRequests;
        let action = app.handle_key(make_key(KeyCode::Char('r'))).unwrap();
        assert!(matches!(action, Action::RefreshMergeRequests));
    }
```

- [ ] **Step 2: Run the new app tests and verify they fail**

Run:

```bash
cargo test mr
```

Expected: FAIL at compile time with missing `View`, MR actions, MR fields, or `Command::RefreshMergeRequests`.

- [ ] **Step 3: Add MR imports and state types**

At the top of `src/app.rs`, add:

```rust
use crate::scm::MergeRequest;
```

Add this enum near `Mode`:

```rust
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum View {
    Agents,
    MergeRequests,
}
```

- [ ] **Step 4: Add actions and command variants**

In `Action`, add:

```rust
    ToggleView,
    RefreshMergeRequests,
    LaunchSelectedMergeRequest,
    MergeRequestsRefreshed(Vec<MergeRequest>),
    MergeRequestsFailed(String),
```

Place `ToggleView` with mode transitions, `RefreshMergeRequests` and `LaunchSelectedMergeRequest` with lifecycle actions, and the result actions with background results.

In `Command`, add:

```rust
    RefreshMergeRequests(Vec<PathBuf>),
```

- [ ] **Step 5: Add MR fields to `App`**

Add these fields to `App`:

```rust
    pub merge_requests: Vec<MergeRequest>,
    pub selected_mr: usize,
    pub view: View,
```

Initialize them in `App::new`:

```rust
            merge_requests: Vec::new(),
            selected_mr: 0,
            view: View::Agents,
```

Add this helper beside `selected_agent`:

```rust
    pub fn selected_merge_request(&self) -> Option<&MergeRequest> {
        self.merge_requests.get(self.selected_mr)
    }
```

- [ ] **Step 6: Update navigation, view toggle, and MR refresh handling**

Change `Action::MoveUp` and `Action::MoveDown` so they dispatch on `self.view`:

```rust
            Action::MoveUp => match self.view {
                View::Agents => {
                    if self.selected > 0 {
                        self.selected -= 1;
                        self.preview_content = None;
                        if let Some(cmd) = self.capture_selected_command() {
                            cmds.push(cmd);
                        }
                    }
                }
                View::MergeRequests => {
                    if self.selected_mr > 0 {
                        self.selected_mr -= 1;
                    }
                }
            },
            Action::MoveDown => match self.view {
                View::Agents => {
                    if self.selected + 1 < self.agents.len() {
                        self.selected += 1;
                        self.preview_content = None;
                        if let Some(cmd) = self.capture_selected_command() {
                            cmds.push(cmd);
                        }
                    }
                }
                View::MergeRequests => {
                    if self.selected_mr + 1 < self.merge_requests.len() {
                        self.selected_mr += 1;
                    }
                }
            },
```

Add these arms in `update`:

```rust
            Action::ToggleView => {
                self.view = match self.view {
                    View::Agents => View::MergeRequests,
                    View::MergeRequests => View::Agents,
                };
                if self.view == View::MergeRequests {
                    cmds.push(Command::RefreshMergeRequests(self.config.resolved_repos()));
                }
            }
            Action::RefreshMergeRequests => {
                cmds.push(Command::RefreshMergeRequests(self.config.resolved_repos()));
            }
            Action::MergeRequestsRefreshed(mrs) => {
                self.merge_requests = mrs;
                if self.selected_mr >= self.merge_requests.len() && !self.merge_requests.is_empty() {
                    self.selected_mr = self.merge_requests.len() - 1;
                }
                if self.merge_requests.is_empty() {
                    self.selected_mr = 0;
                }
            }
            Action::MergeRequestsFailed(error) => {
                self.status_message = Some(format!("MR refresh: {error}"));
            }
```

Leave `Action::LaunchSelectedMergeRequest` as:

```rust
            Action::LaunchSelectedMergeRequest => {}
```

Task 4 replaces that no-op with the launch logic.

- [ ] **Step 7: Update normal-mode key handling**

Replace the `Mode::Normal` key arm with view-aware handling:

```rust
            Mode::Normal => match self.view {
                View::Agents => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
                    KeyCode::Char('j') | KeyCode::Down => Some(Action::MoveDown),
                    KeyCode::Char('k') | KeyCode::Up => Some(Action::MoveUp),
                    KeyCode::Char('m') => Some(Action::ToggleView),
                    KeyCode::Char('n') => Some(Action::StartNewAgent),
                    KeyCode::Char('a') | KeyCode::Enter => Some(Action::Attach),
                    KeyCode::Char('x') => {
                        self.selected_agent()
                            .filter(|a| a.status.has_session())
                            .map(|a| Action::KillSession(a.session_name.clone()))
                    }
                    KeyCode::Char('d') => Some(Action::StartDelete),
                    KeyCode::Char('?') => Some(Action::ToggleHelp),
                    _ => None,
                },
                View::MergeRequests => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
                    KeyCode::Char('j') | KeyCode::Down => Some(Action::MoveDown),
                    KeyCode::Char('k') | KeyCode::Up => Some(Action::MoveUp),
                    KeyCode::Char('m') => Some(Action::ToggleView),
                    KeyCode::Char('r') => Some(Action::RefreshMergeRequests),
                    KeyCode::Char('a') | KeyCode::Enter => Some(Action::LaunchSelectedMergeRequest),
                    KeyCode::Char('?') => Some(Action::ToggleHelp),
                    _ => None,
                },
            },
```

- [ ] **Step 8: Run app tests**

Run:

```bash
cargo test mr
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/app.rs
git commit -m "Add MR inbox state and navigation"
```

## Task 4: Implement manual MR launch by reusing existing agent commands

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add failing launch tests**

Add these tests to `src/app.rs`'s existing `tests` module:

```rust
    #[test]
    fn launch_selected_mr_attaches_matching_running_agent() {
        let mut app = test_app();
        app.view = View::MergeRequests;
        app.merge_requests = vec![mock_merge_request("myapp", "feature/a", 1)];
        app.agents = vec![mock_agent("feature/a")];

        let cmds = app.update(Action::LaunchSelectedMergeRequest);

        assert_eq!(cmds.len(), 1);
        assert!(matches!(cmds[0], Command::Attach(_)));
    }

    #[test]
    fn launch_selected_mr_resumes_matching_stopped_agent() {
        let mut app = test_app();
        app.view = View::MergeRequests;
        app.merge_requests = vec![mock_merge_request("myapp", "feature/a", 1)];
        let mut agent = mock_agent("feature/a");
        agent.status = crate::agent::AgentStatus::Stopped;
        app.agents = vec![agent];

        let cmds = app.update(Action::LaunchSelectedMergeRequest);

        assert_eq!(cmds.len(), 1);
        assert!(matches!(cmds[0], Command::PrepareAttach { .. }));
    }

    #[test]
    fn launch_selected_mr_creates_existing_branch_agent_when_no_local_agent_exists() {
        let mut app = test_app();
        app.view = View::MergeRequests;
        app.merge_requests = vec![mock_merge_request("myapp", "feature/a", 1)];

        let cmds = app.update(Action::LaunchSelectedMergeRequest);

        assert_eq!(app.agents.len(), 1);
        assert_eq!(app.agents[0].branch, "feature/a");
        assert!(matches!(app.agents[0].status, crate::agent::AgentStatus::Creating));
        assert!(matches!(
            &cmds[0],
            Command::CreateAgent {
                branch,
                new_branch: false,
                base_branch: None,
                ..
            } if branch == "feature/a"
        ));
    }
```

- [ ] **Step 2: Run the launch tests and verify they fail**

Run:

```bash
cargo test launch_selected_mr
```

Expected: FAIL because `Action::LaunchSelectedMergeRequest` is still a no-op.

- [ ] **Step 3: Add launch helpers**

Add these helper methods inside `impl App`, near `selected_merge_request`:

```rust
    fn agent_matching_mr(&self, mr: &MergeRequest) -> Option<Agent> {
        self.agents
            .iter()
            .find(|a| a.repo_name == mr.repo_name && a.branch == mr.source_branch)
            .cloned()
    }

    fn repo_path_for_mr(&self, mr: &MergeRequest) -> Option<PathBuf> {
        self.config.resolved_repos().into_iter().find(|repo| {
            repo.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == mr.repo_name)
        })
    }

    fn prepare_attach_command(&mut self, agent: Agent) -> Command {
        let resume_cmd = match self.config.resume(&agent.agent_name) {
            Some(cmd) => cmd,
            None => {
                self.status_message = Some(format!(
                    "agent '{}' not in config \u{2014} using default",
                    agent.agent_name
                ));
                self.config
                    .resume(self.config.default_agent_name())
                    .expect("default_agent is validated to exist in agents")
            }
        };
        Command::PrepareAttach { agent, resume_cmd }
    }
```

- [ ] **Step 4: Refactor `Action::Attach` to use `prepare_attach_command`**

Inside the existing `Action::Attach` arm, replace the stopped-agent resume block with:

```rust
                    if agent.status.has_session() {
                        cmds.push(Command::Attach(agent));
                    } else {
                        if self.status_message.is_none() {
                            self.status_message = Some(format!("Starting: {}", agent.branch));
                        }
                        cmds.push(self.prepare_attach_command(agent));
                    }
```

- [ ] **Step 5: Implement `Action::LaunchSelectedMergeRequest`**

Replace the no-op arm from Task 3 with:

```rust
            Action::LaunchSelectedMergeRequest => {
                let Some(mr) = self.selected_merge_request().cloned() else {
                    self.status_message = Some("No merge request selected".into());
                    return cmds;
                };

                if let Some(agent) = self.agent_matching_mr(&mr) {
                    if agent.status.has_session() {
                        cmds.push(Command::Attach(agent));
                    } else {
                        self.status_message = Some(format!("Starting: {}", agent.branch));
                        cmds.push(self.prepare_attach_command(agent));
                    }
                    return cmds;
                }

                let Some(repo) = self.repo_path_for_mr(&mr) else {
                    self.status_message = Some(format!("Repo not configured: {}", mr.repo_name));
                    return cmds;
                };

                let agent_name = self.config.default_agent_name().to_string();
                let fresh_cmd = self
                    .config
                    .fresh(&agent_name, None)
                    .expect("default_agent is validated to exist in agents");
                let session_name = agent::session_name(&mr.repo_name, &mr.source_branch);
                let slug = mr.source_branch.replace('/', "-");

                self.agents.push(Agent {
                    repo_path: repo.clone(),
                    repo_name: mr.repo_name.clone(),
                    branch: mr.source_branch.clone(),
                    base_branch: Some(mr.target_branch.clone()),
                    worktree_path: PathBuf::new(),
                    slug,
                    session_name: session_name.clone(),
                    status: AgentStatus::Creating,
                    agent_name: agent_name.clone(),
                    last_pane_hash: None,
                    last_attached_count: None,
                    quiet_captures: 0,
                    seen_activity_since_seed: false,
                    was_spinner_visible: false,
                    consecutive_emits: 0,
                });

                cmds.push(Command::CreateAgent {
                    repo,
                    branch: mr.source_branch,
                    new_branch: false,
                    base_branch: None,
                    session_name,
                    agent_name,
                    fresh_cmd,
                });
            }
```

- [ ] **Step 6: Run launch and attach regression tests**

Run:

```bash
cargo test launch_selected_mr
cargo test attach
```

Expected: both commands exit 0.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs
git commit -m "Launch agents from merge requests"
```

## Task 5: Execute MR refresh commands in `main.rs`

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add the provider import**

Near the existing imports in `src/main.rs`, add:

```rust
use scm::ScmProvider;
```

- [ ] **Step 2: Implement command execution**

In `execute`, add this arm before `Command::Attach(_)`:

```rust
        Command::RefreshMergeRequests(repos) => {
            let tx = tx.clone();
            tokio::spawn(async move {
                let provider = scm::GitLabScm;
                let mut all = Vec::new();
                let mut errors = Vec::new();

                for repo_path in repos {
                    match scm::scm_repo_from_path(&repo_path).await {
                        Ok(repo) => match provider.list_open_merge_requests(&repo).await {
                            Ok(mut mrs) => all.append(&mut mrs),
                            Err(err) => errors.push(err.to_string()),
                        },
                        Err(err) => errors.push(err.to_string()),
                    }
                }

                if !all.is_empty() || errors.is_empty() {
                    let _ = tx.send(Action::MergeRequestsRefreshed(all));
                }
                if !errors.is_empty() {
                    let _ = tx.send(Action::MergeRequestsFailed(errors.join("; ")));
                }
            });
        }
```

- [ ] **Step 3: Run a build**

Run:

```bash
cargo build
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "Refresh merge requests from GitLab"
```

## Task 6: Render the MR inbox view

**Files:**
- Modify: `src/ui.rs`

- [ ] **Step 1: Add failing unit tests for MR state labels**

Add this test module to the end of `src/ui.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scm::MergeRequestState;

    #[test]
    fn mr_state_labels_are_short() {
        assert_eq!(mr_state_label(MergeRequestState::Draft), "draft");
        assert_eq!(mr_state_label(MergeRequestState::CiFailed), "ci_fail");
        assert_eq!(mr_state_label(MergeRequestState::Review), "review");
        assert_eq!(mr_state_label(MergeRequestState::Ready), "ready");
        assert_eq!(mr_state_label(MergeRequestState::Unknown), "unknown");
    }
}
```

- [ ] **Step 2: Run the UI label test and verify it fails**

Run:

```bash
cargo test ui::tests::mr_state_labels_are_short
```

Expected: FAIL with missing `mr_state_label`.

- [ ] **Step 3: Add imports and MR label helper**

At the top of `src/ui.rs`, change the app import:

```rust
use crate::app::{App, Mode, View};
```

Add this SCM import:

```rust
use crate::scm::{MergeRequest, MergeRequestState};
```

Add this helper near `status_glyph`:

```rust
fn mr_state_label(state: MergeRequestState) -> &'static str {
    match state {
        MergeRequestState::Draft => "draft",
        MergeRequestState::CiFailed => "ci_fail",
        MergeRequestState::Review => "review",
        MergeRequestState::Ready => "ready",
        MergeRequestState::Unknown => "unknown",
    }
}
```

- [ ] **Step 4: Dispatch table rendering by view**

In `draw`, replace the direct table call:

```rust
    draw_agent_table(frame, app, chunks[4]);
```

with:

```rust
    match app.view {
        View::Agents => draw_agent_table(frame, app, chunks[4]),
        View::MergeRequests => draw_merge_request_table(frame, app, chunks[4]),
    }
```

- [ ] **Step 5: Add MR table rendering**

Add this helper below `draw_agent_table`:

```rust
fn matching_agent_for_mr<'a>(app: &'a App, mr: &MergeRequest) -> Option<&'a Agent> {
    app.agents
        .iter()
        .find(|a| a.repo_name == mr.repo_name && a.branch == mr.source_branch)
}

fn draw_merge_request_table(frame: &mut Frame, app: &App, area: Rect) {
    if app.merge_requests.is_empty() {
        let msg = "No open merge requests.";
        let line = Line::from(Span::styled(msg, Style::default().fg(DIM)));
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    let visible_rows = (area.height as usize).saturating_sub(2);
    let offset = if visible_rows == 0 {
        0
    } else if app.selected_mr >= visible_rows {
        app.selected_mr - visible_rows + 1
    } else {
        0
    };

    let branch_w = app
        .merge_requests
        .iter()
        .map(|mr| mr.source_branch.len() + 4 + mr.target_branch.len())
        .max()
        .unwrap_or(0)
        .max(14) as u16;
    let title_w = app
        .merge_requests
        .iter()
        .map(|mr| mr.title.len())
        .max()
        .unwrap_or(0)
        .max(5) as u16;
    let repo_w = app
        .merge_requests
        .iter()
        .map(|mr| mr.repo_name.len())
        .max()
        .unwrap_or(0)
        .max(4) as u16;

    let mut rows = Vec::new();
    for (i, mr) in app.merge_requests.iter().enumerate().skip(offset).take(visible_rows) {
        let is_selected = i == app.selected_mr;
        let text_style = if is_selected {
            Style::default().fg(TEXT)
        } else {
            Style::default().fg(DIM)
        };
        let indicator = if is_selected { "\u{2502}" } else { " " };
        let indicator_style = if is_selected {
            Style::default().fg(TEXT)
        } else {
            Style::default()
        };
        let local_status = matching_agent_for_mr(app, mr)
            .map(|agent| status_glyph(agent, app.spinner_frame, text_style))
            .unwrap_or_else(|| Span::styled(" ", Style::default()));
        let branch = format!("{} -> {}", mr.source_branch, mr.target_branch);
        let iid = format!("!{}", mr.iid);

        rows.push(Row::new(vec![
            Cell::from(Span::styled(indicator, indicator_style)),
            Cell::from(local_status),
            Cell::from(Span::styled(mr_state_label(mr.state), text_style)),
            Cell::from(Span::styled(iid, text_style)),
            Cell::from(Span::styled(branch, text_style)),
            Cell::from(Span::styled(mr.title.as_str(), text_style)),
            Cell::from(Span::styled(mr.repo_name.as_str(), text_style)),
        ]));
    }

    let hdr_style = Style::default().fg(DIM);
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from(""),
        Cell::from(Span::styled("STATE", hdr_style)),
        Cell::from(Span::styled("MR", hdr_style)),
        Cell::from(Span::styled("BRANCH", hdr_style)),
        Cell::from(Span::styled("TITLE", hdr_style)),
        Cell::from(Span::styled("REPO", hdr_style)),
    ])
    .bottom_margin(1);

    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(branch_w + 2),
            Constraint::Min(title_w),
            Constraint::Length(repo_w + 2),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::NONE));

    frame.render_widget(table, area);
}
```

- [ ] **Step 6: Update preview and separator for MR view**

Replace `draw_preview` with:

```rust
fn draw_preview(frame: &mut Frame, app: &App, area: Rect) {
    match app.view {
        View::Agents => {
            let content = app.preview_content.as_deref().unwrap_or("");
            let tail = tail_lines(content.trim_end(), area.height as usize);
            let preview = Paragraph::new(tail).style(Style::default().fg(TEXT));
            frame.render_widget(preview, area);
        }
        View::MergeRequests => {
            let lines = app
                .selected_merge_request()
                .map(|mr| {
                    vec![
                        Line::from(Span::styled(mr.title.as_str(), Style::default().fg(TEXT))),
                        Line::from(Span::styled(mr.web_url.as_str(), Style::default().fg(DIM))),
                    ]
                })
                .unwrap_or_default();
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
        }
    }
}
```

At the top of `draw_separator`, add this branch:

```rust
    if app.view == View::MergeRequests {
        let label = app
            .selected_merge_request()
            .map(|mr| format!(" !{} {} ", mr.iid, mr.source_branch))
            .unwrap_or_else(|| " merge requests ".to_string());
        let label_len = label.chars().count();
        let left_dashes = 3usize;
        let right_dashes = w.saturating_sub(left_dashes + label_len);
        let line = Line::from(vec![
            Span::styled("\u{2500}".repeat(left_dashes), dash_style),
            Span::styled(label, Style::default().fg(TEXT)),
            Span::styled("\u{2500}".repeat(right_dashes), dash_style),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }
```

- [ ] **Step 7: Update status bar hints by view**

Replace `draw_status_bar` with:

```rust
fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let line = if let Some(msg) = &app.status_message {
        Line::from(Span::styled(msg.as_str(), Style::default().fg(DIM)))
    } else if app.help_visible {
        match app.view {
            View::Agents => footer_hint(&[
                ("\u{2191}/k", "up"),
                ("\u{2193}/j", "down"),
                ("m", "mrs"),
                ("n", "new"),
                ("a", "attach"),
                ("x", "stop"),
                ("d", "delete"),
                ("?", "hide"),
                ("q", "quit"),
            ]),
            View::MergeRequests => footer_hint(&[
                ("\u{2191}/k", "up"),
                ("\u{2193}/j", "down"),
                ("a", "launch"),
                ("r", "refresh"),
                ("m", "agents"),
                ("?", "hide"),
                ("q", "quit"),
            ]),
        }
    } else {
        Line::from(Span::styled("?", Style::default().fg(DIM)))
    };
    frame.render_widget(Paragraph::new(line), area);
}
```

- [ ] **Step 8: Run UI test and full test suite**

Run:

```bash
cargo test ui::tests::mr_state_labels_are_short
cargo test
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/ui.rs
git commit -m "Render merge request inbox"
```

## Task 7: Final verification and manual smoke test

**Files:**
- No source changes expected.

- [ ] **Step 1: Format**

Run:

```bash
cargo fmt
```

Expected: command exits 0.

- [ ] **Step 2: Run full tests**

Run:

```bash
cargo test
```

Expected: PASS.

- [ ] **Step 3: Run build**

Run:

```bash
cargo build
```

Expected: PASS.

- [ ] **Step 4: Manual TUI smoke**

Run:

```bash
cargo run
```

Expected:
- TUI opens.
- `?` shows agent help with `m mrs`.
- `m` switches to Merge Requests.
- If configured repos are GitLab repos and `glab` is authenticated, MR rows appear.
- If configured repos are not GitLab repos or `glab` cannot authenticate, the app stays open and shows an MR refresh status message.
- `r` refreshes in the MR view.
- `m` returns to Agents.
- `q` exits.

- [ ] **Step 5: Commit formatting fixes**

Run:

```bash
git status --short
```

If `cargo fmt` changed files, commit them:

```bash
git add Cargo.toml Cargo.lock src/main.rs src/scm.rs src/app.rs src/ui.rs
git commit -m "Format SCM MR inbox"
```

If `git status --short` prints nothing, do not create a commit.
