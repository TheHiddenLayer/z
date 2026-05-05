# Agentic GitLab MR Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a stateless, glab-backed GitLab MR workflow to `z`, including compact MR state and selected agent-backed intent commands.

**Architecture:** Add one focused `src/gitlab.rs` module for normalized MR data, `glab` argv builders, permissive JSON parsing, MR display classification, and intent prompt builders. Extend `App` with an in-memory MR snapshot map and preview mode; extend `main.rs` command execution to run `glab` and start agent-backed intent sessions without adding any persistent task store.

**Tech Stack:** Rust 2024, tokio process execution, ratatui/crossterm, tmux, git, `glab` CLI, `serde_json` for permissive JSON parsing.

---

## File Structure

- Create `src/gitlab.rs`
  - Owns MR data model, parser, argv builders, display classification, prompt builders, and tests.
  - No side effects except pure argv construction and JSON parsing.

- Modify `Cargo.toml`
  - Add `serde_json = "1"` for parsing `glab --output json`.

- Modify `src/main.rs`
  - Add `mod gitlab;`.
  - Execute new `Command` variants by spawning `glab`.
  - Start agent-backed intent sessions with existing `agent::create_session`.

- Modify `src/app.rs`
  - Add MR snapshot state and preview mode.
  - Add MR actions, command variants, update logic, key bindings, and tests.
  - Keep all MR state in memory.

- Modify `src/ui.rs`
  - Render compact MR glyphs in the agent table.
  - Render MR preview when selected preview mode is MR.
  - Render state-sensitive MR footer hints.
  - Add merge confirmation modal.

- Modify `src/style.rs`
  - No new colors. Existing `OK`, `BUSY`, `FAIL`, `DIM`, and `TEXT` are reused.

---

### Task 1: Add GitLab MR Model, Parser, And Display Classification

**Files:**
- Create: `src/gitlab.rs`
- Modify: `Cargo.toml`
- Modify: `src/main.rs`

- [ ] **Step 1: Add JSON dependency**

Edit `Cargo.toml` and add `serde_json` under `[dependencies]`:

```toml
serde_json = "1"
```

- [ ] **Step 2: Register the module**

Edit the module list at the top of `src/main.rs`:

```rust
mod config;
mod agent;
mod app;
mod gitlab;
mod notifications;
mod style;
mod ui;
```

- [ ] **Step 3: Write failing parser and classifier tests**

Create `src/gitlab.rs` with the tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_mr_list_returns_none() {
        assert_eq!(parse_mr_list("[]").unwrap(), None);
    }

    #[test]
    fn parse_open_mr_list_item() {
        let json = r#"[{
            "iid": 42,
            "title": "Add cache",
            "web_url": "https://gitlab.example.com/g/r/-/merge_requests/42",
            "source_branch": "feature/cache",
            "target_branch": "main",
            "state": "opened",
            "draft": false,
            "detailed_merge_status": "mergeable",
            "pipeline": { "status": "success" },
            "user_notes_count": 3
        }]"#;

        let mr = parse_mr_list(json).unwrap().unwrap();
        assert_eq!(mr.iid, Some(42));
        assert_eq!(mr.title.as_deref(), Some("Add cache"));
        assert_eq!(mr.url.as_deref(), Some("https://gitlab.example.com/g/r/-/merge_requests/42"));
        assert_eq!(mr.source_branch, "feature/cache");
        assert_eq!(mr.target_branch.as_deref(), Some("main"));
        assert_eq!(mr.state, MrState::Open);
        assert_eq!(mr.draft, Some(false));
        assert_eq!(mr.merge_state.as_deref(), Some("mergeable"));
        assert_eq!(mr.pipeline_state.as_deref(), Some("success"));
    }

    #[test]
    fn parse_view_object_accepts_camel_case_fields() {
        let json = r#"{
            "iid": "7",
            "title": "Fix drift",
            "webUrl": "https://gitlab.example.com/g/r/-/merge_requests/7",
            "sourceBranch": "fix/drift",
            "targetBranch": "master",
            "state": "merged",
            "work_in_progress": false,
            "mergeStatus": "can_be_merged"
        }"#;

        let mr = parse_mr_view(json).unwrap();
        assert_eq!(mr.iid, Some(7));
        assert_eq!(mr.source_branch, "fix/drift");
        assert_eq!(mr.target_branch.as_deref(), Some("master"));
        assert_eq!(mr.state, MrState::Merged);
        assert_eq!(mr.merge_state.as_deref(), Some("can_be_merged"));
    }

    #[test]
    fn malformed_json_returns_error() {
        let err = parse_mr_list("{").unwrap_err();
        assert!(err.contains("json"));
    }

    #[test]
    fn missing_branch_degrades_to_unknown_branch() {
        let json = r#"[{ "iid": 1, "state": "opened" }]"#;
        let mr = parse_mr_list(json).unwrap().unwrap();
        assert_eq!(mr.source_branch, "");
        assert_eq!(mr.state, MrState::Open);
    }

    #[test]
    fn classify_no_mr_is_blank_dim() {
        assert_eq!(classify(None).glyph, " ");
        assert_eq!(classify(None).kind, MrDisplayKind::None);
    }

    #[test]
    fn classify_draft_is_d() {
        let mut mr = mr("feature/x");
        mr.draft = Some(true);
        let d = classify(Some(&mr));
        assert_eq!(d.glyph, "D");
        assert_eq!(d.kind, MrDisplayKind::Draft);
    }

    #[test]
    fn classify_ready_open_is_r() {
        let mut mr = mr("feature/x");
        mr.merge_state = Some("mergeable".into());
        mr.pipeline_state = Some("success".into());
        let d = classify(Some(&mr));
        assert_eq!(d.glyph, "R");
        assert_eq!(d.kind, MrDisplayKind::Ready);
    }

    #[test]
    fn classify_blocked_is_b() {
        let mut mr = mr("feature/x");
        mr.merge_state = Some("cannot_be_merged".into());
        let d = classify(Some(&mr));
        assert_eq!(d.glyph, "B");
        assert_eq!(d.kind, MrDisplayKind::Blocked);
    }

    #[test]
    fn classify_merged_is_check() {
        let mut mr = mr("feature/x");
        mr.state = MrState::Merged;
        let d = classify(Some(&mr));
        assert_eq!(d.glyph, "\u{2713}");
        assert_eq!(d.kind, MrDisplayKind::Merged);
    }

    fn mr(source_branch: &str) -> MergeRequest {
        MergeRequest {
            source_branch: source_branch.into(),
            target_branch: Some("main".into()),
            iid: Some(1),
            title: Some("Title".into()),
            url: Some("https://gitlab.example.com/g/r/-/merge_requests/1".into()),
            state: MrState::Open,
            draft: Some(false),
            merge_state: None,
            pipeline_state: None,
            unresolved_count: None,
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run:

```bash
cargo test gitlab::
```

Expected: compile failure because the tested types and functions are not defined.

- [ ] **Step 5: Implement the model and parser**

Replace the top of `src/gitlab.rs` above the test module with:

```rust
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MrState {
    None,
    Open,
    Closed,
    Merged,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeRequest {
    pub source_branch: String,
    pub target_branch: Option<String>,
    pub iid: Option<u64>,
    pub title: Option<String>,
    pub url: Option<String>,
    pub state: MrState,
    pub draft: Option<bool>,
    pub merge_state: Option<String>,
    pub pipeline_state: Option<String>,
    pub unresolved_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MrDisplayKind {
    None,
    Unknown,
    Draft,
    Ready,
    Blocked,
    Open,
    Merged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MrDisplay {
    pub glyph: &'static str,
    pub kind: MrDisplayKind,
}

pub fn parse_mr_list(output: &str) -> Result<Option<MergeRequest>, String> {
    let value: Value = serde_json::from_str(output)
        .map_err(|e| format!("json parse failed: {e}"))?;
    let Some(items) = value.as_array() else {
        return Err("json root is not an array".into());
    };
    Ok(items.first().map(parse_mr_value))
}

pub fn parse_mr_view(output: &str) -> Result<MergeRequest, String> {
    let value: Value = serde_json::from_str(output)
        .map_err(|e| format!("json parse failed: {e}"))?;
    Ok(parse_mr_value(&value))
}

pub fn classify(mr: Option<&MergeRequest>) -> MrDisplay {
    let Some(mr) = mr else {
        return MrDisplay { glyph: " ", kind: MrDisplayKind::None };
    };

    if matches!(mr.state, MrState::Merged) {
        return MrDisplay { glyph: "\u{2713}", kind: MrDisplayKind::Merged };
    }
    if !matches!(mr.state, MrState::Open) {
        return MrDisplay { glyph: "!", kind: MrDisplayKind::Unknown };
    }
    if mr.draft == Some(true) {
        return MrDisplay { glyph: "D", kind: MrDisplayKind::Draft };
    }

    let merge = mr.merge_state.as_deref().unwrap_or("").to_ascii_lowercase();
    let pipe = mr.pipeline_state.as_deref().unwrap_or("").to_ascii_lowercase();
    let blocked = [
        "cannot_be_merged",
        "conflict",
        "checking",
        "unchecked",
        "blocked_status",
        "ci_must_pass",
    ]
    .iter()
    .any(|needle| merge.contains(needle))
        || matches!(pipe.as_str(), "failed" | "canceled" | "cancelled" | "skipped");
    if blocked {
        return MrDisplay { glyph: "B", kind: MrDisplayKind::Blocked };
    }

    let ready_merge = matches!(merge.as_str(), "mergeable" | "can_be_merged");
    let ready_pipeline = pipe.is_empty() || matches!(pipe.as_str(), "success" | "passed");
    if ready_merge && ready_pipeline {
        return MrDisplay { glyph: "R", kind: MrDisplayKind::Ready };
    }

    MrDisplay { glyph: "R", kind: MrDisplayKind::Open }
}

fn parse_mr_value(v: &Value) -> MergeRequest {
    MergeRequest {
        source_branch: read_string_any(v, &["source_branch", "sourceBranch"])
            .unwrap_or_default(),
        target_branch: read_string_any(v, &["target_branch", "targetBranch"]),
        iid: read_u64_any(v, &["iid", "id"]),
        title: read_string_any(v, &["title"]),
        url: read_string_any(v, &["web_url", "webUrl", "url"]),
        state: parse_state(read_string_any(v, &["state"]).as_deref()),
        draft: read_bool_any(v, &["draft", "work_in_progress", "workInProgress"]),
        merge_state: read_string_any(v, &[
            "detailed_merge_status",
            "detailedMergeStatus",
            "merge_status",
            "mergeStatus",
            "merge_state",
            "mergeState",
        ]),
        pipeline_state: read_pipeline_state(v),
        unresolved_count: read_u32_any(v, &[
            "unresolved_discussions_count",
            "unresolvedDiscussionsCount",
            "blocking_discussions_resolved",
            "user_notes_count",
        ]),
    }
}

fn parse_state(raw: Option<&str>) -> MrState {
    match raw.unwrap_or("").to_ascii_lowercase().as_str() {
        "" => MrState::None,
        "open" | "opened" => MrState::Open,
        "closed" => MrState::Closed,
        "merged" => MrState::Merged,
        other => MrState::Unknown(other.to_string()),
    }
}

fn read_pipeline_state(v: &Value) -> Option<String> {
    read_string_any(v, &["pipeline_status", "pipelineStatus"]).or_else(|| {
        v.get("pipeline")
            .and_then(|p| read_string_any(p, &["status"]))
    })
}

fn read_string_any(v: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(value) = v.get(*key) else { continue };
        if let Some(s) = value.as_str().map(str::trim).filter(|s| !s.is_empty()) {
            return Some(s.to_string());
        }
        if let Some(n) = value.as_u64() {
            return Some(n.to_string());
        }
    }
    None
}

fn read_u64_any(v: &Value, keys: &[&str]) -> Option<u64> {
    for key in keys {
        let Some(value) = v.get(*key) else { continue };
        if let Some(n) = value.as_u64() {
            return Some(n);
        }
        if let Some(s) = value.as_str()
            && let Ok(n) = s.trim().parse()
        {
            return Some(n);
        }
    }
    None
}

fn read_u32_any(v: &Value, keys: &[&str]) -> Option<u32> {
    read_u64_any(v, keys).and_then(|n| u32::try_from(n).ok())
}

fn read_bool_any(v: &Value, keys: &[&str]) -> Option<bool> {
    for key in keys {
        let Some(value) = v.get(*key) else { continue };
        if let Some(b) = value.as_bool() {
            return Some(b);
        }
        if let Some(s) = value.as_str() {
            match s.trim().to_ascii_lowercase().as_str() {
                "true" => return Some(true),
                "false" => return Some(false),
                _ => {}
            }
        }
    }
    None
}
```

- [ ] **Step 6: Run tests**

Run:

```bash
cargo test gitlab::
```

Expected: all `gitlab::tests` pass.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs src/gitlab.rs
git commit -m "feat: add GitLab MR parsing"
```

---

### Task 2: Add glab Argv Builders And Agent Intent Prompts

**Files:**
- Modify: `src/gitlab.rs`

- [ ] **Step 1: Write failing tests for argv builders and prompts**

Append these tests inside `src/gitlab.rs`'s existing test module:

```rust
#[test]
fn list_args_are_argv_safe() {
    assert_eq!(
        list_args("feature/a b"),
        vec!["mr", "list", "--source-branch", "feature/a b", "--output", "json"],
    );
}

#[test]
fn create_args_include_source_target_fill_and_yes() {
    assert_eq!(
        create_args("feature/x", "main"),
        vec![
            "mr",
            "create",
            "--fill",
            "--source-branch",
            "feature/x",
            "--target-branch",
            "main",
            "--yes",
        ],
    );
}

#[test]
fn merge_args_confirm_direct_merge() {
    assert_eq!(
        merge_args("feature/x"),
        vec!["mr", "merge", "feature/x", "--yes"],
    );
}

#[test]
fn prompt_rebase_forbids_merge_and_names_target() {
    let prompt = rebase_prompt("main");
    assert!(prompt.contains("Rebase this worktree's branch onto main."));
    assert!(prompt.contains("do not merge the merge request"));
}

#[test]
fn prompt_make_ready_names_url_and_pushes() {
    let prompt = make_ready_prompt("https://gitlab.example/mr/1");
    assert!(prompt.contains("Make this GitLab merge request ready to merge"));
    assert!(prompt.contains("https://gitlab.example/mr/1"));
    assert!(prompt.contains("push the branch when changes are ready"));
    assert!(prompt.contains("do not merge the merge request"));
}

#[test]
fn prompt_review_fix_mentions_unresolved_feedback() {
    let prompt = review_fix_prompt("https://gitlab.example/mr/2");
    assert!(prompt.contains("Address unresolved review feedback"));
    assert!(prompt.contains("inspect unresolved discussions/comments"));
    assert!(prompt.contains("do not merge the merge request"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test gitlab::
```

Expected: compile failure for missing `list_args`, `create_args`, `merge_args`, `rebase_prompt`, `make_ready_prompt`, and `review_fix_prompt`.

- [ ] **Step 3: Implement argv builders and prompts**

Add this code above the test module in `src/gitlab.rs`:

```rust
pub fn list_args(source_branch: &str) -> Vec<String> {
    strings(&["mr", "list", "--source-branch", source_branch, "--output", "json"])
}

pub fn view_args(id_or_branch: &str) -> Vec<String> {
    strings(&["mr", "view", id_or_branch, "--output", "json"])
}

pub fn view_comments_args(id_or_branch: &str) -> Vec<String> {
    strings(&["mr", "view", id_or_branch, "--comments", "--unresolved", "--output", "json"])
}

pub fn create_args(source_branch: &str, target_branch: &str) -> Vec<String> {
    strings(&[
        "mr",
        "create",
        "--fill",
        "--source-branch",
        source_branch,
        "--target-branch",
        target_branch,
        "--yes",
    ])
}

pub fn open_args(id_or_branch: &str) -> Vec<String> {
    strings(&["mr", "view", id_or_branch, "--web"])
}

pub fn merge_args(id_or_branch: &str) -> Vec<String> {
    strings(&["mr", "merge", id_or_branch, "--yes"])
}

pub fn note_args(id_or_branch: &str, message: &str) -> Vec<String> {
    strings(&["mr", "note", id_or_branch, "--message", message])
}

pub fn rebase_prompt(target_branch: &str) -> String {
    format!(
        "\
Rebase this worktree's branch onto {target_branch}.

Requirements:
- fetch the latest refs first
- perform the rebase in this worktree
- resolve conflicts while preserving the intended changes on this branch
- run the relevant focused validation for the files you touched
- do not merge the merge request
- stop with a concise summary of what changed and what validation ran"
    )
}

pub fn make_ready_prompt(mr_url: &str) -> String {
    format!(
        "\
Make this GitLab merge request ready to merge: {mr_url}.

Requirements:
- inspect MR status, CI/check failures, branch-behind state, conflicts, and unresolved review feedback
- update this worktree branch as needed
- fix issues required for merge readiness
- run relevant validation
- push the branch when changes are ready
- do not merge the merge request
- stop with a concise summary and any remaining blocker"
    )
}

pub fn review_fix_prompt(mr_url: &str) -> String {
    format!(
        "\
Address unresolved review feedback on this GitLab merge request: {mr_url}.

Requirements:
- inspect unresolved discussions/comments
- make the requested code changes in this worktree
- run relevant validation
- push the branch when changes are ready
- do not merge the merge request
- stop with a concise summary of addressed feedback and remaining threads"
    )
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_string()).collect()
}
```

- [ ] **Step 4: Run tests**

Run:

```bash
cargo test gitlab::
```

Expected: all `gitlab::tests` pass.

- [ ] **Step 5: Commit**

```bash
git add src/gitlab.rs
git commit -m "feat: build glab MR commands"
```

---

### Task 3: Add App MR State And Refresh Command

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add imports and data types**

At the top of `src/app.rs`, replace the first imports with:

```rust
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::agent::{self, Agent, AgentStatus};
use crate::config::Config;
use crate::gitlab::{MergeRequest, MrDisplayKind, classify};
use crate::notifications;
```

Add these definitions after `NewAgentFocus`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MrKey {
    pub repo_path: PathBuf,
    pub branch: String,
}

impl MrKey {
    pub fn new(repo_path: PathBuf, branch: String) -> Self {
        Self { repo_path, branch }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MrSnapshot {
    Missing,
    Ready(MergeRequest),
    Error(String),
    Refreshing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewMode {
    Terminal,
    MergeRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MrIntent {
    Rebase,
    MakeReady,
    ReviewFix,
}
```

- [ ] **Step 2: Extend `Action`, `Command`, and `Mode`**

Add these `Action` variants after `BranchesLoaded`:

```rust
    TogglePreview,
    RefreshMrs,
    MrRefreshed {
        key: MrKey,
        snapshot: MrSnapshot,
    },
    MrCreate,
    MrOpen,
    MrMerge,
    MrMergeConfirmed,
    MrIntent(MrIntent),
```

Add these `Command` variants after `PrepareAttach`:

```rust
    RefreshMr {
        key: MrKey,
        source_branch: String,
    },
    CreateMr {
        key: MrKey,
        source_branch: String,
        target_branch: String,
    },
    OpenMr {
        id_or_branch: String,
    },
    MergeMr {
        key: MrKey,
        id_or_branch: String,
    },
    StartAgentIntent {
        agent: Agent,
        fresh_cmd: String,
    },
```

Add a new mode variant:

```rust
    ConfirmMerge,
```

- [ ] **Step 3: Extend `App` state**

Add fields to `App`:

```rust
    pub preview_mode: PreviewMode,
    pub mr_snapshots: HashMap<MrKey, MrSnapshot>,
    mr_refresh_pending: bool,
```

Initialize them in `App::new`:

```rust
            preview_mode: PreviewMode::Terminal,
            mr_snapshots: HashMap::new(),
            mr_refresh_pending: false,
```

- [ ] **Step 4: Add helper methods**

Add these methods inside `impl App` before `update`:

```rust
    pub fn selected_mr_key(&self) -> Option<MrKey> {
        let agent = self.selected_agent()?;
        Some(MrKey::new(agent.repo_path.clone(), agent.branch.clone()))
    }

    pub fn selected_mr_snapshot(&self) -> Option<&MrSnapshot> {
        let key = self.selected_mr_key()?;
        self.mr_snapshots.get(&key)
    }

    pub fn mr_for_agent(&self, agent: &Agent) -> Option<&MergeRequest> {
        let key = MrKey::new(agent.repo_path.clone(), agent.branch.clone());
        match self.mr_snapshots.get(&key) {
            Some(MrSnapshot::Ready(mr)) => Some(mr),
            _ => None,
        }
    }

    pub fn selected_mr(&self) -> Option<&MergeRequest> {
        match self.selected_mr_snapshot() {
            Some(MrSnapshot::Ready(mr)) => Some(mr),
            _ => None,
        }
    }

    pub fn selected_mr_id_or_branch(&self) -> Option<String> {
        let mr = self.selected_mr()?;
        Some(mr.iid.map(|iid| iid.to_string()).unwrap_or_else(|| mr.source_branch.clone()))
    }

    fn refresh_mr_commands(&self) -> Vec<Command> {
        self.agents
            .iter()
            .map(|agent| {
                let key = MrKey::new(agent.repo_path.clone(), agent.branch.clone());
                Command::RefreshMr {
                    key,
                    source_branch: agent.branch.clone(),
                }
            })
            .collect()
    }

    fn selected_agent_fresh_cmd(&self, prompt: &str) -> Option<String> {
        let agent = self.selected_agent()?;
        self.config.fresh(&agent.agent_name, Some(prompt))
            .or_else(|| self.config.fresh(self.config.default_agent_name(), Some(prompt)))
    }

    fn selected_base_branch(&self) -> String {
        self.selected_agent()
            .and_then(|a| a.base_branch.clone())
            .or_else(|| self.selected_mr().and_then(|mr| mr.target_branch.clone()))
            .unwrap_or_else(|| "main".to_string())
    }
```

- [ ] **Step 5: Write failing app tests**

Append these tests in `src/app.rs`'s test module:

```rust
#[test]
fn refresh_agents_also_requests_mr_refresh() {
    let mut app = test_app();
    let cmds = app.update(Action::RefreshAgents);
    assert!(cmds.iter().any(|c| matches!(c, Command::Discover(_))));
}

#[test]
fn agents_refreshed_requests_one_mr_refresh_per_agent() {
    let mut app = test_app();
    let cmds = app.update(Action::AgentsRefreshed(vec![mock_agent("fix-auth"), mock_agent("docs")]));
    let count = cmds.iter().filter(|c| matches!(c, Command::RefreshMr { .. })).count();
    assert_eq!(count, 2);
}

#[test]
fn mr_refreshed_stores_snapshot_by_repo_and_branch() {
    let mut app = test_app();
    let key = MrKey::new("/tmp/repo".into(), "fix-auth".into());
    app.update(Action::MrRefreshed {
        key: key.clone(),
        snapshot: MrSnapshot::Missing,
    });
    assert_eq!(app.mr_snapshots.get(&key), Some(&MrSnapshot::Missing));
}

#[test]
fn toggle_preview_switches_between_terminal_and_mr() {
    let mut app = test_app();
    assert_eq!(app.preview_mode, PreviewMode::Terminal);
    app.update(Action::TogglePreview);
    assert_eq!(app.preview_mode, PreviewMode::MergeRequest);
    app.update(Action::TogglePreview);
    assert_eq!(app.preview_mode, PreviewMode::Terminal);
}

#[test]
fn m_creates_mr_when_selected_agent_has_no_mr() {
    let mut app = test_app();
    app.agents = vec![mock_agent("fix-auth")];
    let cmds = app.update(Action::MrCreate);
    assert!(matches!(
        cmds.as_slice(),
        [Command::CreateMr { source_branch, target_branch, .. }]
            if source_branch == "fix-auth" && target_branch == "main"
    ));
}

#[test]
fn m_switches_to_mr_preview_when_mr_exists() {
    let mut app = test_app();
    app.agents = vec![mock_agent("fix-auth")];
    let key = app.selected_mr_key().unwrap();
    app.mr_snapshots.insert(key, MrSnapshot::Ready(test_mr("fix-auth")));
    let cmds = app.update(Action::MrCreate);
    assert!(cmds.is_empty());
    assert_eq!(app.preview_mode, PreviewMode::MergeRequest);
}

#[test]
fn merge_refuses_non_ready_mr() {
    let mut app = test_app();
    app.agents = vec![mock_agent("fix-auth")];
    let key = app.selected_mr_key().unwrap();
    let mut mr = test_mr("fix-auth");
    mr.merge_state = Some("cannot_be_merged".into());
    app.mr_snapshots.insert(key, MrSnapshot::Ready(mr));
    let cmds = app.update(Action::MrMerge);
    assert!(cmds.is_empty());
    assert_eq!(app.status_message.as_deref(), Some("not ready; use f make-ready"));
}

#[test]
fn merge_ready_mr_enters_confirmation() {
    let mut app = test_app();
    app.agents = vec![mock_agent("fix-auth")];
    let key = app.selected_mr_key().unwrap();
    let mut mr = test_mr("fix-auth");
    mr.merge_state = Some("mergeable".into());
    mr.pipeline_state = Some("success".into());
    app.mr_snapshots.insert(key, MrSnapshot::Ready(mr));
    let cmds = app.update(Action::MrMerge);
    assert!(cmds.is_empty());
    assert!(matches!(app.mode, Mode::ConfirmMerge));
}

#[test]
fn running_agent_intent_is_refused() {
    let mut app = test_app();
    app.agents = vec![mock_agent("fix-auth")];
    let cmds = app.update(Action::MrIntent(MrIntent::Rebase));
    assert!(cmds.is_empty());
    assert_eq!(app.status_message.as_deref(), Some("agent running; attach or stop first"));
}

#[test]
fn stopped_agent_intent_starts_session() {
    let mut app = test_app();
    let mut agent = mock_agent("fix-auth");
    agent.status = AgentStatus::Stopped;
    app.agents = vec![agent];
    let cmds = app.update(Action::MrIntent(MrIntent::Rebase));
    assert!(matches!(cmds.as_slice(), [Command::StartAgentIntent { fresh_cmd, .. }] if fresh_cmd.contains("Rebase this worktree")));
}

fn test_mr(branch: &str) -> MergeRequest {
    MergeRequest {
        source_branch: branch.to_string(),
        target_branch: Some("main".into()),
        iid: Some(1),
        title: Some("MR".into()),
        url: Some("https://gitlab.example.com/mr/1".into()),
        state: crate::gitlab::MrState::Open,
        draft: Some(false),
        merge_state: None,
        pipeline_state: None,
        unresolved_count: None,
    }
}
```

- [ ] **Step 6: Run tests to verify failure**

Run:

```bash
cargo test app::
```

Expected: compile failures until update logic is implemented.

- [ ] **Step 7: Implement update logic**

Add these arms inside `App::update` before `Action::Tick`:

```rust
            Action::TogglePreview => {
                self.preview_mode = match self.preview_mode {
                    PreviewMode::Terminal => PreviewMode::MergeRequest,
                    PreviewMode::MergeRequest => PreviewMode::Terminal,
                };
            }
            Action::RefreshMrs => {
                if !self.mr_refresh_pending {
                    self.mr_refresh_pending = true;
                    for cmd in self.refresh_mr_commands() {
                        cmds.push(cmd);
                    }
                }
            }
            Action::MrRefreshed { key, snapshot } => {
                self.mr_snapshots.insert(key, snapshot);
                self.mr_refresh_pending = false;
            }
            Action::MrCreate => {
                if self.selected_mr().is_some() {
                    self.preview_mode = PreviewMode::MergeRequest;
                } else if let Some(agent) = self.selected_agent() {
                    let key = MrKey::new(agent.repo_path.clone(), agent.branch.clone());
                    cmds.push(Command::CreateMr {
                        key,
                        source_branch: agent.branch.clone(),
                        target_branch: self.selected_base_branch(),
                    });
                }
            }
            Action::MrOpen => {
                if let Some(id_or_branch) = self.selected_mr_id_or_branch() {
                    cmds.push(Command::OpenMr { id_or_branch });
                } else {
                    self.status_message = Some("no MR".into());
                }
            }
            Action::MrMerge => {
                let ready = self
                    .selected_mr()
                    .map(|mr| classify(Some(mr)).kind == MrDisplayKind::Ready)
                    .unwrap_or(false);
                if ready {
                    self.mode = Mode::ConfirmMerge;
                } else {
                    self.status_message = Some("not ready; use f make-ready".into());
                }
            }
            Action::MrMergeConfirmed => {
                if let (Some(key), Some(id_or_branch)) =
                    (self.selected_mr_key(), self.selected_mr_id_or_branch())
                {
                    cmds.push(Command::MergeMr { key, id_or_branch });
                }
                self.mode = Mode::Normal;
            }
            Action::MrIntent(intent) => {
                if let Some(agent) = self.selected_agent().cloned() {
                    match agent.status {
                        AgentStatus::Running => {
                            self.status_message = Some("agent running; attach or stop first".into());
                        }
                        AgentStatus::Creating => {
                            self.status_message = Some("agent still creating".into());
                        }
                        AgentStatus::Error(_) => {
                            self.status_message = Some("agent errored; delete or restart first".into());
                        }
                        AgentStatus::Stopped => {
                            let prompt = match intent {
                                MrIntent::Rebase => {
                                    crate::gitlab::rebase_prompt(&self.selected_base_branch())
                                }
                                MrIntent::MakeReady => {
                                    let url = self
                                        .selected_mr()
                                        .and_then(|mr| mr.url.as_deref())
                                        .unwrap_or("selected merge request");
                                    crate::gitlab::make_ready_prompt(url)
                                }
                                MrIntent::ReviewFix => {
                                    let url = self
                                        .selected_mr()
                                        .and_then(|mr| mr.url.as_deref())
                                        .unwrap_or("selected merge request");
                                    crate::gitlab::review_fix_prompt(url)
                                }
                            };
                            if let Some(fresh_cmd) = self.selected_agent_fresh_cmd(&prompt) {
                                cmds.push(Command::StartAgentIntent { agent, fresh_cmd });
                            } else {
                                self.status_message = Some("agent command unavailable".into());
                            }
                        }
                    }
                }
            }
```

In `Action::AgentsRefreshed`, after `self.agents = new_agents;`, add:

```rust
                if !self.mr_refresh_pending {
                    self.mr_refresh_pending = true;
                    for cmd in self.refresh_mr_commands() {
                        cmds.push(cmd);
                    }
                }
```

In `Action::Tick`, after agent rediscovery scheduling, add a light MR refresh:

```rust
                if self.spinner_frame.is_multiple_of(100) && !self.mr_refresh_pending {
                    self.mr_refresh_pending = true;
                    for cmd in self.refresh_mr_commands() {
                        cmds.push(cmd);
                    }
                }
```

- [ ] **Step 8: Run tests**

Run:

```bash
cargo test app::
```

Expected: tests pass or fail only for key handling that Task 4 adds.

- [ ] **Step 9: Commit**

```bash
git add src/app.rs
git commit -m "feat: track MR state in app"
```

---

### Task 4: Add Key Bindings And Merge Confirmation Mode

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Write failing key tests**

Append these tests in `src/app.rs`'s test module:

```rust
#[test]
fn normal_tab_toggles_preview() {
    let app = test_app();
    let action = app.handle_key(make_key(KeyCode::Tab));
    assert!(matches!(action, Some(Action::TogglePreview)));
}

#[test]
fn normal_m_maps_to_mr_create_or_view() {
    let app = test_app();
    let action = app.handle_key(make_key(KeyCode::Char('m')));
    assert!(matches!(action, Some(Action::MrCreate)));
}

#[test]
fn normal_upper_m_maps_to_merge() {
    let app = test_app();
    let action = app.handle_key(make_key(KeyCode::Char('M')));
    assert!(matches!(action, Some(Action::MrMerge)));
}

#[test]
fn normal_o_maps_to_open_mr() {
    let app = test_app();
    let action = app.handle_key(make_key(KeyCode::Char('o')));
    assert!(matches!(action, Some(Action::MrOpen)));
}

#[test]
fn normal_r_maps_to_agentic_rebase() {
    let app = test_app();
    let action = app.handle_key(make_key(KeyCode::Char('r')));
    assert!(matches!(action, Some(Action::MrIntent(MrIntent::Rebase))));
}

#[test]
fn normal_f_maps_to_make_ready() {
    let app = test_app();
    let action = app.handle_key(make_key(KeyCode::Char('f')));
    assert!(matches!(action, Some(Action::MrIntent(MrIntent::MakeReady))));
}

#[test]
fn normal_v_maps_to_review_fix() {
    let app = test_app();
    let action = app.handle_key(make_key(KeyCode::Char('v')));
    assert!(matches!(action, Some(Action::MrIntent(MrIntent::ReviewFix))));
}

#[test]
fn confirm_merge_y_confirms() {
    let mut app = test_app();
    app.mode = Mode::ConfirmMerge;
    let action = app.handle_key(make_key(KeyCode::Char('y')));
    assert!(matches!(action, Some(Action::MrMergeConfirmed)));
}

#[test]
fn confirm_merge_escape_cancels() {
    let mut app = test_app();
    app.mode = Mode::ConfirmMerge;
    let action = app.handle_key(make_key(KeyCode::Esc));
    assert!(matches!(action, Some(Action::CancelMode)));
}
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test app::
```

Expected: key tests fail until `handle_key` is updated.

- [ ] **Step 3: Implement key bindings**

In `handle_key`, update `Mode::Normal`:

```rust
            Mode::Normal => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
                KeyCode::Char('j') | KeyCode::Down => Some(Action::MoveDown),
                KeyCode::Char('k') | KeyCode::Up => Some(Action::MoveUp),
                KeyCode::Char('n') => Some(Action::StartNewAgent),
                KeyCode::Char('a') | KeyCode::Enter => Some(Action::Attach),
                KeyCode::Char('m') => Some(Action::MrCreate),
                KeyCode::Char('M') => Some(Action::MrMerge),
                KeyCode::Char('o') => Some(Action::MrOpen),
                KeyCode::Char('r') => Some(Action::MrIntent(MrIntent::Rebase)),
                KeyCode::Char('f') => Some(Action::MrIntent(MrIntent::MakeReady)),
                KeyCode::Char('v') => Some(Action::MrIntent(MrIntent::ReviewFix)),
                KeyCode::Tab => Some(Action::TogglePreview),
                KeyCode::Char('x') => {
                    self.selected_agent()
                        .filter(|a| a.status.has_session())
                        .map(|a| Action::KillSession(a.session_name.clone()))
                }
                KeyCode::Char('d') => Some(Action::StartDelete),
                KeyCode::Char('?') => Some(Action::ToggleHelp),
                _ => None,
            },
```

Add a new match arm for `Mode::ConfirmMerge`:

```rust
            Mode::ConfirmMerge => match key.code {
                KeyCode::Esc => Some(Action::CancelMode),
                KeyCode::Char('q') => Some(Action::CancelMode),
                KeyCode::Char('y') => Some(Action::MrMergeConfirmed),
                _ => None,
            },
```

- [ ] **Step 4: Run tests**

Run:

```bash
cargo test app::
```

Expected: app tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat: add MR workflow keys"
```

---

### Task 5: Execute glab Commands And Agent-Backed Intents

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add helper functions for glab output**

Add these helper functions above `fn execute`:

```rust
fn stderr_tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.lines().rev().find(|l| !l.trim().is_empty())
        .unwrap_or("command failed")
        .trim()
        .to_string()
}

async fn run_glab(args: Vec<String>) -> Result<String, String> {
    let output = tokio::process::Command::new("glab")
        .args(args)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "glab not found".to_string()
            } else {
                format!("glab failed: {e}")
            }
        })?;
    if !output.status.success() {
        let tail = stderr_tail(&output.stderr);
        if tail.to_ascii_lowercase().contains("authentication")
            || tail.to_ascii_lowercase().contains("not authenticated")
            || tail.to_ascii_lowercase().contains("login")
        {
            return Err("glab auth required".into());
        }
        return Err(tail);
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
```

- [ ] **Step 2: Extend `execute` for MR command variants**

Add these match arms inside `fn execute` before `Command::Attach(_)`:

```rust
        Command::RefreshMr { key, source_branch } => {
            let tx = tx.clone();
            tokio::spawn(async move {
                let snapshot = match run_glab(crate::gitlab::list_args(&source_branch)).await {
                    Ok(stdout) => match crate::gitlab::parse_mr_list(&stdout) {
                        Ok(Some(mr)) => crate::app::MrSnapshot::Ready(mr),
                        Ok(None) => crate::app::MrSnapshot::Missing,
                        Err(e) => crate::app::MrSnapshot::Error(e),
                    },
                    Err(e) => crate::app::MrSnapshot::Error(e),
                };
                let _ = tx.send(Action::MrRefreshed { key, snapshot });
            });
        }
        Command::CreateMr { key, source_branch, target_branch } => {
            let tx = tx.clone();
            tokio::spawn(async move {
                let snapshot = match run_glab(crate::gitlab::create_args(&source_branch, &target_branch)).await {
                    Ok(_) => match run_glab(crate::gitlab::list_args(&source_branch)).await {
                        Ok(stdout) => match crate::gitlab::parse_mr_list(&stdout) {
                            Ok(Some(mr)) => crate::app::MrSnapshot::Ready(mr),
                            Ok(None) => crate::app::MrSnapshot::Missing,
                            Err(e) => crate::app::MrSnapshot::Error(e),
                        },
                        Err(e) => crate::app::MrSnapshot::Error(e),
                    },
                    Err(e) => crate::app::MrSnapshot::Error(format!("MR create: {e}")),
                };
                let _ = tx.send(Action::MrRefreshed { key, snapshot });
            });
        }
        Command::OpenMr { id_or_branch } => {
            tokio::spawn(async move {
                let _ = run_glab(crate::gitlab::open_args(&id_or_branch)).await;
            });
        }
        Command::MergeMr { key, id_or_branch } => {
            let tx = tx.clone();
            tokio::spawn(async move {
                let snapshot = match run_glab(crate::gitlab::merge_args(&id_or_branch)).await {
                    Ok(_) => match run_glab(crate::gitlab::view_args(&id_or_branch)).await {
                        Ok(stdout) => match crate::gitlab::parse_mr_view(&stdout) {
                            Ok(mr) => crate::app::MrSnapshot::Ready(mr),
                            Err(e) => crate::app::MrSnapshot::Error(e),
                        },
                        Err(e) => crate::app::MrSnapshot::Error(e),
                    },
                    Err(e) => crate::app::MrSnapshot::Error(format!("MR merge: {e}")),
                };
                let _ = tx.send(Action::MrRefreshed { key, snapshot });
            });
        }
        Command::StartAgentIntent { agent, fresh_cmd } => {
            let tx = tx.clone();
            tokio::spawn(async move {
                match agent::create_session(&agent.session_name, &agent.worktree_path, Some(&fresh_cmd)).await {
                    Ok(()) => {
                        let _ = tx.send(Action::RefreshAgents);
                    }
                    Err(e) => {
                        let _ = tx.send(Action::AgentFailed {
                            session: agent.session_name.clone(),
                            error: e,
                        });
                    }
                }
            });
        }
```

- [ ] **Step 3: Run tests**

Run:

```bash
cargo test app:: gitlab::
```

Expected: app and gitlab tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs src/app.rs
git commit -m "feat: execute GitLab MR actions"
```

---

### Task 6: Render MR Glyphs, MR Preview, Footer Hints, And Merge Modal

**Files:**
- Modify: `src/ui.rs`
- Modify: `src/style.rs` only if import ordering requires formatting

- [ ] **Step 1: Add UI helper functions**

In `src/ui.rs`, extend imports:

```rust
use crate::app::{App, Mode, MrSnapshot, PreviewMode};
use crate::agent::{Agent, AgentStatus};
use crate::gitlab::{MergeRequest, MrDisplayKind, MrState, classify};
```

Add helper functions after `status_glyph`:

```rust
fn mr_glyph(app: &App, agent: &Agent) -> Span<'static> {
    let display = classify(app.mr_for_agent(agent));
    let color = match display.kind {
        MrDisplayKind::None => DIM,
        MrDisplayKind::Unknown | MrDisplayKind::Blocked => crate::style::FAIL,
        MrDisplayKind::Draft | MrDisplayKind::Open => crate::style::BUSY,
        MrDisplayKind::Ready | MrDisplayKind::Merged => crate::style::OK,
    };
    Span::styled(display.glyph, Style::default().fg(color))
}

fn mr_preview_lines(snapshot: Option<&MrSnapshot>) -> Vec<Line<'static>> {
    match snapshot {
        Some(MrSnapshot::Ready(mr)) => render_mr(mr),
        Some(MrSnapshot::Missing) | None => vec![
            Line::from(Span::styled("No merge request", Style::default().fg(DIM))),
            Line::from(Span::styled("m create MR", Style::default().fg(DIM))),
        ],
        Some(MrSnapshot::Refreshing) => vec![
            Line::from(Span::styled("Refreshing merge request...", Style::default().fg(DIM))),
        ],
        Some(MrSnapshot::Error(e)) => vec![
            Line::from(Span::styled("Merge request unavailable", Style::default().fg(crate::style::FAIL))),
            Line::from(Span::styled(e.clone(), Style::default().fg(DIM))),
        ],
    }
}

fn render_mr(mr: &MergeRequest) -> Vec<Line<'static>> {
    let id = mr.iid.map(|n| format!("!{n}")).unwrap_or_else(|| "MR".into());
    let title = mr.title.clone().unwrap_or_else(|| "(untitled)".into());
    let state = match &mr.state {
        MrState::None => "none".to_string(),
        MrState::Open if mr.draft == Some(true) => "draft".to_string(),
        MrState::Open => "open".to_string(),
        MrState::Closed => "closed".to_string(),
        MrState::Merged => "merged".to_string(),
        MrState::Unknown(s) => s.clone(),
    };
    let target = mr.target_branch.clone().unwrap_or_else(|| "?".into());
    let merge = mr.merge_state.clone().unwrap_or_else(|| "unknown".into());
    let pipe = mr.pipeline_state.clone().unwrap_or_else(|| "unknown".into());
    let url = mr.url.clone().unwrap_or_default();

    vec![
        Line::from(vec![
            Span::styled(id, Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().fg(DIM)),
            Span::styled(title, Style::default().fg(TEXT)),
        ]),
        Line::from(Span::styled(
            format!("{} -> {} · {}", mr.source_branch, target, state),
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            format!("merge: {merge} · pipeline: {pipe}"),
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(url, Style::default().fg(DIM))),
    ]
}
```

- [ ] **Step 2: Render MR preview**

Replace `draw_preview` with:

```rust
fn draw_preview(frame: &mut Frame, app: &App, area: Rect) {
    if app.preview_mode == PreviewMode::MergeRequest {
        let lines = mr_preview_lines(app.selected_mr_snapshot());
        frame.render_widget(Paragraph::new(lines).style(Style::default().fg(TEXT)), area);
        return;
    }

    let content = app.preview_content.as_deref().unwrap_or("");
    let tail = tail_lines(content.trim_end(), area.height as usize);

    let preview = Paragraph::new(tail)
        .style(Style::default().fg(TEXT));

    frame.render_widget(preview, area);
}
```

- [ ] **Step 3: Add MR column to agent table**

In `draw_agent_table`, add MR column to rows after status:

```rust
            Cell::from(status_glyph(agent, app.spinner_frame, text_style)),
            Cell::from(mr_glyph(app, agent)),
            Cell::from(branch_cell),
```

Update the header:

```rust
        Cell::from(""),
        Cell::from(""),
        Cell::from(Span::styled("BRANCH", hdr_style)),
```

becomes:

```rust
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(Span::styled("BRANCH", hdr_style)),
```

Update constraints by inserting `Constraint::Length(2)` after status:

```rust
            Constraint::Length(1),
            Constraint::Length(status_w + 1),
            Constraint::Length(2),
            Constraint::Length(branch_w + 2),
```

- [ ] **Step 4: Render MR-aware footer hints**

Replace the final `else` in `draw_status_bar` with:

```rust
    } else if app.selected_agent().is_some() {
        match app.selected_mr_snapshot() {
            Some(MrSnapshot::Ready(mr)) => {
                let kind = classify(Some(mr)).kind;
                match kind {
                    MrDisplayKind::Ready => footer_hint(&[
                        ("m", "MR"),
                        ("M", "merge"),
                        ("o", "open"),
                        ("r", "rebase"),
                        ("tab", "preview"),
                    ]),
                    MrDisplayKind::Blocked | MrDisplayKind::Draft | MrDisplayKind::Open => {
                        footer_hint(&[
                            ("f", "make-ready"),
                            ("r", "rebase"),
                            ("v", "review-fix"),
                            ("o", "open"),
                            ("tab", "preview"),
                        ])
                    }
                    _ => footer_hint(&[("m", "MR"), ("o", "open"), ("tab", "preview")]),
                }
            }
            Some(MrSnapshot::Error(_)) => footer_hint(&[("m", "retry"), ("tab", "preview")]),
            _ => footer_hint(&[("m", "create MR"), ("tab", "preview"), ("?", "help")]),
        }
    } else {
        Line::from(Span::styled("?", Style::default().fg(DIM)))
    };
```

- [ ] **Step 5: Add merge confirmation modal**

In `draw`, add a `Mode::ConfirmMerge` arm:

```rust
        Mode::ConfirmMerge => {
            let modal_area = centered_rect(52, 24, frame.area());
            frame.render_widget(Clear, modal_area);
            draw_merge_modal(frame, app, modal_area);
        }
```

Add this function after `draw_delete_modal`:

```rust
fn draw_merge_modal(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(modal_title("Merge MR"));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let title = app
        .selected_mr()
        .and_then(|mr| mr.title.as_deref())
        .unwrap_or("selected merge request");

    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled("  Merge", Style::default().fg(TEXT)))),
        chunks[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  ", Style::default().fg(TEXT)),
            Span::styled(title.to_string(), Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
            Span::styled("?", Style::default().fg(TEXT)),
        ])),
        chunks[2],
    );

    let mut spans = vec![Span::raw("  ")];
    spans.extend(footer_hint(&[("y", "merge"), ("q/esc", "cancel")]).spans);
    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[4]);
}
```

- [ ] **Step 6: Run tests and formatting check**

Run:

```bash
cargo test
cargo fmt --check
```

Expected: tests pass; format check may fail if files need formatting.

- [ ] **Step 7: Format if needed, then rerun**

Run:

```bash
cargo fmt
cargo test
cargo fmt --check
```

Expected: tests pass and format check passes.

- [ ] **Step 8: Commit**

```bash
git add src/ui.rs src/style.rs
git commit -m "feat: show GitLab MR state"
```

---

### Task 7: Polish Status Messages And Full Validation

**Files:**
- Modify: `src/main.rs`
- Modify: `src/app.rs`
- Modify: `src/gitlab.rs`
- Modify: `src/ui.rs`

- [ ] **Step 1: Add final regression tests**

Add these tests where the corresponding functions live:

In `src/gitlab.rs` tests:

```rust
#[test]
fn classify_failed_pipeline_blocks_merge() {
    let mut mr = mr("feature/x");
    mr.merge_state = Some("mergeable".into());
    mr.pipeline_state = Some("failed".into());
    assert_eq!(classify(Some(&mr)).kind, MrDisplayKind::Blocked);
}
```

In `src/app.rs` tests:

```rust
#[test]
fn create_mr_uses_mr_target_branch_when_base_missing() {
    let mut app = test_app();
    let mut agent = mock_agent("fix-auth");
    agent.base_branch = None;
    app.agents = vec![agent];
    let key = app.selected_mr_key().unwrap();
    let mut mr = test_mr("fix-auth");
    mr.target_branch = Some("develop".into());
    app.mr_snapshots.insert(key, MrSnapshot::Ready(mr));
    let cmds = app.update(Action::MrCreate);
    assert!(matches!(
        cmds.as_slice(),
        [Command::CreateMr { target_branch, .. }] if target_branch == "develop"
    ));
}
```

- [ ] **Step 2: Run final tests**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 3: Run formatting and lint-equivalent checks**

Run:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: both commands pass. If `clippy` is unavailable in the toolchain, record that in the final handoff and rely on `cargo test` plus `cargo fmt --check`.

- [ ] **Step 4: Manual smoke test with no GitLab remote**

Run:

```bash
cargo run
```

Expected:

- UI opens.
- Existing agent rows still render.
- Rows without GitLab MR data do not show noisy errors.
- `?` still shows help.
- `tab` toggles between terminal and MR preview.
- `q` exits.

- [ ] **Step 5: Manual smoke test with authenticated glab repo**

In a repo configured in `~/.config/z/config.toml` that has a GitLab remote and an authenticated `glab` session, run:

```bash
glab auth status
cargo run
```

Expected:

- An existing MR branch shows an MR glyph.
- `tab` shows MR details.
- `o` opens the MR in a browser.
- `r` on a stopped agent starts a tmux session with the rebase intent.
- `r` on a running agent shows `agent running; attach or stop first`.

- [ ] **Step 6: Commit final polish**

```bash
git add src/main.rs src/app.rs src/gitlab.rs src/ui.rs Cargo.toml Cargo.lock
git commit -m "test: cover GitLab MR workflow"
```

---

## Implementation Notes

- Keep `glab` operations async and never block the render loop.
- Keep MR state in `App` only. Do not add a config file, JSON cache, or database.
- Do not add a new visual accent color.
- Do not inject prompts into running tmux sessions.
- Preserve existing `destroy`, attach, new-agent, and notification behavior.
- Prefer `glab` argv vectors over shell strings everywhere.

## Final Verification

Run before claiming implementation complete:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
git status --short
```

Expected:

- formatting passes
- tests pass
- clippy passes or the toolchain limitation is documented
- worktree contains only intentional changes
