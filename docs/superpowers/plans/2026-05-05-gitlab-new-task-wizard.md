# GitLab New Task Wizard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a progressive New Agent wizard that can start work from assigned GitLab issues, GitLab MRs needing review, or the existing branch flow.

**Architecture:** Add a focused `src/gitlab.rs` module for `glab` JSON parsing, prompt generation, and command wrappers. Extend the existing `Mode::NewAgent` state machine in `src/app.rs` instead of adding a separate modal. Keep `src/ui.rs` as a renderer over app state; async side effects stay behind `Command` execution in `src/main.rs`.

**Tech Stack:** Rust 2024, ratatui 0.29, crossterm 0.28, tokio process execution, serde/serde_json for `glab --output json`.

**Spec reference:** `docs/superpowers/specs/2026-05-05-gitlab-new-task-wizard-design.md`

---

## File Structure

| File | Status | Responsibility |
|------|--------|----------------|
| `Cargo.toml` | modify | Add `serde_json` for parsing `glab --output json`. |
| `src/main.rs` | modify | Register `mod gitlab`; execute new GitLab list/MR-branch commands. |
| `src/gitlab.rs` | create | GitLab structs, JSON parsing, prompt generation, branch naming, async `glab` list wrappers. |
| `src/agent.rs` | modify | Add GitLab MR refspec helper and async branch preparation without switching the main worktree. |
| `src/app.rs` | modify | Extend wizard state, actions, commands, key handling, fetch result handling, prompt/source behavior, and create-command generation. |
| `src/ui.rs` | modify | Render progressive source flow, issue/MR pickers, loading/empty/error rows, and prompt edit hints. |
| `docs/superpowers/plans/2026-05-05-gitlab-new-task-wizard.md` | create | This plan. |

---

## Task 1: GitLab Data Model And Parsing

**Files:**
- Modify: `Cargo.toml`
- Create: `src/gitlab.rs`
- Modify: `src/main.rs:1-6`

- [ ] **Step 1.1: Add the JSON dependency**

In `Cargo.toml`, add `serde_json = "1"` under `[dependencies]`:

```toml
serde_json = "1"
```

- [ ] **Step 1.2: Register the new module**

In `src/main.rs`, add the module next to the other local modules:

```rust
mod config;
mod agent;
mod app;
mod gitlab;
mod notifications;
mod style;
mod ui;
```

- [ ] **Step 1.3: Write failing parser tests**

Create `src/gitlab.rs` with the tests first:

```rust
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct GitlabIssue {
    pub iid: u64,
    pub title: String,
    pub description: Option<String>,
    pub web_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GitlabMergeRequest {
    pub iid: u64,
    pub title: String,
    pub description: Option<String>,
    pub web_url: Option<String>,
    pub source_branch: String,
    pub target_branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GitlabError {
    Command(String),
    Json(String),
    MissingField(&'static str),
}

pub fn parse_issues(_json: &str) -> Result<Vec<GitlabIssue>, GitlabError> {
    unimplemented!("implemented in Step 1.5")
}

pub fn parse_merge_requests(_json: &str) -> Result<Vec<GitlabMergeRequest>, GitlabError> {
    unimplemented!("implemented in Step 1.5")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_issue_list_accepts_glab_json() {
        let json = r#"
        [
          {
            "iid": 1102,
            "title": "Detect agents remotely",
            "description": "Use remote shell profiles.",
            "web_url": "https://gitlab.example.com/acme/example/-/issues/1102"
          }
        ]
        "#;

        let issues = parse_issues(json).unwrap();

        assert_eq!(
            issues,
            vec![GitlabIssue {
                iid: 1102,
                title: "Detect agents remotely".to_string(),
                description: Some("Use remote shell profiles.".to_string()),
                web_url: Some("https://gitlab.example.com/acme/example/-/issues/1102".to_string()),
            }]
        );
    }

    #[test]
    fn parse_issue_list_accepts_camel_case_url() {
        let json = r#"[{"iid":7,"title":"Small fix","webUrl":"https://gitlab/x/y/-/issues/7"}]"#;

        let issues = parse_issues(json).unwrap();

        assert_eq!(issues[0].web_url.as_deref(), Some("https://gitlab/x/y/-/issues/7"));
    }

    #[test]
    fn parse_mr_list_accepts_glab_json() {
        let json = r#"
        [
          {
            "iid": 184,
            "title": "Use remote shell profiles",
            "description": "Review remote profile detection.",
            "web_url": "https://gitlab.example.com/acme/example/-/merge_requests/184",
            "source_branch": "fix/remote-shell-profiles",
            "target_branch": "main"
          }
        ]
        "#;

        let mrs = parse_merge_requests(json).unwrap();

        assert_eq!(
            mrs,
            vec![GitlabMergeRequest {
                iid: 184,
                title: "Use remote shell profiles".to_string(),
                description: Some("Review remote profile detection.".to_string()),
                web_url: Some("https://gitlab.example.com/acme/example/-/merge_requests/184".to_string()),
                source_branch: "fix/remote-shell-profiles".to_string(),
                target_branch: Some("main".to_string()),
            }]
        );
    }

    #[test]
    fn parse_mr_list_accepts_camel_case_branches() {
        let json = r#"
        [
          {
            "iid": 9,
            "title": "Review me",
            "sourceBranch": "feature/review-me",
            "targetBranch": "develop",
            "webUrl": "https://gitlab/x/y/-/merge_requests/9"
          }
        ]
        "#;

        let mrs = parse_merge_requests(json).unwrap();

        assert_eq!(mrs[0].source_branch, "feature/review-me");
        assert_eq!(mrs[0].target_branch.as_deref(), Some("develop"));
        assert_eq!(mrs[0].web_url.as_deref(), Some("https://gitlab/x/y/-/merge_requests/9"));
    }

    #[test]
    fn parse_issue_list_rejects_missing_iid() {
        let err = parse_issues(r#"[{"title":"No iid"}]"#).unwrap_err();

        assert_eq!(err, GitlabError::MissingField("iid"));
    }

    #[test]
    fn parse_mr_list_rejects_missing_source_branch() {
        let err = parse_merge_requests(r#"[{"iid":1,"title":"No branch"}]"#).unwrap_err();

        assert_eq!(err, GitlabError::MissingField("source_branch"));
    }
}
```

- [ ] **Step 1.4: Run parser tests to verify failure**

Run:

```bash
cargo test gitlab::tests::parse_issue_list_accepts_glab_json
```

Expected: fail because `parse_issues` is not implemented.

- [ ] **Step 1.5: Implement parsing**

Replace the stubs in `src/gitlab.rs` with:

```rust
#[derive(Debug, Deserialize)]
struct RawIssue {
    iid: Option<u64>,
    title: Option<String>,
    description: Option<String>,
    #[serde(alias = "webUrl")]
    web_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawMergeRequest {
    iid: Option<u64>,
    title: Option<String>,
    description: Option<String>,
    #[serde(alias = "webUrl")]
    web_url: Option<String>,
    #[serde(alias = "sourceBranch")]
    source_branch: Option<String>,
    #[serde(alias = "targetBranch")]
    target_branch: Option<String>,
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

impl From<serde_json::Error> for GitlabError {
    fn from(value: serde_json::Error) -> Self {
        GitlabError::Json(value.to_string())
    }
}

pub fn parse_issues(json: &str) -> Result<Vec<GitlabIssue>, GitlabError> {
    let raw: Vec<RawIssue> = serde_json::from_str(json)?;
    raw.into_iter()
        .map(|issue| {
            Ok(GitlabIssue {
                iid: issue.iid.ok_or(GitlabError::MissingField("iid"))?,
                title: issue.title.ok_or(GitlabError::MissingField("title"))?,
                description: clean_optional(issue.description),
                web_url: clean_optional(issue.web_url),
            })
        })
        .collect()
}

pub fn parse_merge_requests(json: &str) -> Result<Vec<GitlabMergeRequest>, GitlabError> {
    let raw: Vec<RawMergeRequest> = serde_json::from_str(json)?;
    raw.into_iter()
        .map(|mr| {
            Ok(GitlabMergeRequest {
                iid: mr.iid.ok_or(GitlabError::MissingField("iid"))?,
                title: mr.title.ok_or(GitlabError::MissingField("title"))?,
                description: clean_optional(mr.description),
                web_url: clean_optional(mr.web_url),
                source_branch: mr
                    .source_branch
                    .ok_or(GitlabError::MissingField("source_branch"))?,
                target_branch: clean_optional(mr.target_branch),
            })
        })
        .collect()
}
```

- [ ] **Step 1.6: Verify parsing tests pass**

Run:

```bash
cargo test gitlab::tests
```

Expected: all `gitlab::tests` pass.

- [ ] **Step 1.7: Verify full suite**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 1.8: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs src/gitlab.rs
git commit -m "Add GitLab JSON parsing"
```

---

## Task 2: Prompt And Branch Helpers

**Files:**
- Modify: `src/gitlab.rs`

- [ ] **Step 2.1: Add failing helper tests**

Append these tests inside `src/gitlab.rs`'s existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn issue_prompt_includes_title_url_and_description() {
    let issue = GitlabIssue {
        iid: 1102,
        title: "Detect agents remotely".to_string(),
        description: Some("Use remote shell profiles.".to_string()),
        web_url: Some("https://gitlab/x/y/-/issues/1102".to_string()),
    };

    assert_eq!(
        issue_prompt(&issue),
        "Work on GitLab issue #1102: Detect agents remotely\nhttps://gitlab/x/y/-/issues/1102\n\nUse remote shell profiles."
    );
}

#[test]
fn mr_prompt_includes_branches_when_target_exists() {
    let mr = GitlabMergeRequest {
        iid: 184,
        title: "Use remote shell profiles".to_string(),
        description: Some("Review remote profile detection.".to_string()),
        web_url: Some("https://gitlab/x/y/-/merge_requests/184".to_string()),
        source_branch: "fix/remote-shell-profiles".to_string(),
        target_branch: Some("main".to_string()),
    };

    assert_eq!(
        mr_prompt(&mr),
        "Review GitLab MR !184: Use remote shell profiles\nfix/remote-shell-profiles -> main\nhttps://gitlab/x/y/-/merge_requests/184\n\nReview remote profile detection."
    );
}

#[test]
fn issue_branch_name_uses_date_number_and_slug() {
    let issue = GitlabIssue {
        iid: 1102,
        title: "Detect agents remotely!".to_string(),
        description: None,
        web_url: None,
    };

    assert_eq!(
        issue_branch_name(&issue, "0505", &[]),
        "z-0505-1102-detect-agents-remotely"
    );
}

#[test]
fn issue_branch_name_appends_collision_suffix() {
    let issue = GitlabIssue {
        iid: 1102,
        title: "Detect agents remotely".to_string(),
        description: None,
        web_url: None,
    };
    let branches = vec![
        "z-0505-1102-detect-agents-remotely".to_string(),
        "z-0505-1102-detect-agents-remotely-2".to_string(),
    ];

    assert_eq!(
        issue_branch_name(&issue, "0505", &branches),
        "z-0505-1102-detect-agents-remotely-3"
    );
}

#[test]
fn issue_branch_name_truncates_long_titles_before_collision_suffix() {
    let issue = GitlabIssue {
        iid: 1,
        title: "a very long issue title that should not create a ridiculous branch name".to_string(),
        description: None,
        web_url: None,
    };

    let branch = issue_branch_name(&issue, "0505", &[]);

    assert!(branch.len() <= 64, "branch was too long: {branch}");
    assert!(branch.starts_with("z-0505-1-a-very-long-issue-title"));
}
```

- [ ] **Step 2.2: Run one helper test to verify failure**

Run:

```bash
cargo test gitlab::tests::issue_prompt_includes_title_url_and_description
```

Expected: fail because `issue_prompt` is not defined.

- [ ] **Step 2.3: Implement helpers**

Append this implementation above the test module in `src/gitlab.rs`:

```rust
fn push_non_empty(parts: &mut Vec<String>, value: Option<&str>) {
    if let Some(value) = value {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }
}

pub fn issue_prompt(issue: &GitlabIssue) -> String {
    let mut parts = vec![format!("Work on GitLab issue #{}: {}", issue.iid, issue.title)];
    push_non_empty(&mut parts, issue.web_url.as_deref());
    if let Some(description) = issue.description.as_deref().filter(|s| !s.trim().is_empty()) {
        parts.push(String::new());
        parts.push(description.trim().to_string());
    }
    parts.join("\n")
}

pub fn mr_prompt(mr: &GitlabMergeRequest) -> String {
    let mut parts = vec![format!("Review GitLab MR !{}: {}", mr.iid, mr.title)];
    let branch_line = match mr.target_branch.as_deref() {
        Some(target) if !target.trim().is_empty() => {
            format!("{} -> {}", mr.source_branch, target.trim())
        }
        _ => mr.source_branch.clone(),
    };
    parts.push(branch_line);
    push_non_empty(&mut parts, mr.web_url.as_deref());
    if let Some(description) = mr.description.as_deref().filter(|s| !s.trim().is_empty()) {
        parts.push(String::new());
        parts.push(description.trim().to_string());
    }
    parts.join("\n")
}

fn slug_title(title: &str) -> String {
    let mut out = String::new();
    let mut previous_dash = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            out.push('-');
            previous_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

pub fn issue_branch_name(issue: &GitlabIssue, date_str: &str, branches: &[String]) -> String {
    let slug = slug_title(&issue.title);
    let prefix = format!("z-{date_str}-{}-", issue.iid);
    let max_slug_len = 64usize.saturating_sub(prefix.len()).max(1);
    let mut trimmed_slug = slug.chars().take(max_slug_len).collect::<String>();
    trimmed_slug = trimmed_slug.trim_matches('-').to_string();
    if trimmed_slug.is_empty() {
        trimmed_slug = "issue".to_string();
    }

    let base = format!("{prefix}{trimmed_slug}");
    if !branches.iter().any(|b| b == &base) {
        return base;
    }

    for n in 2.. {
        let suffix = format!("-{n}");
        let allowed = 64usize.saturating_sub(suffix.len());
        let candidate_base = if base.len() > allowed {
            base.chars()
                .take(allowed)
                .collect::<String>()
                .trim_matches('-')
                .to_string()
        } else {
            base.clone()
        };
        let candidate = format!("{candidate_base}{suffix}");
        if !branches.iter().any(|b| b == &candidate) {
            return candidate;
        }
    }
    unreachable!("unbounded suffix search must return")
}
```

- [ ] **Step 2.4: Run helper tests**

Run:

```bash
cargo test gitlab::tests
```

Expected: all `gitlab::tests` pass.

- [ ] **Step 2.5: Run full suite**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 2.6: Commit**

```bash
git add src/gitlab.rs
git commit -m "Add GitLab prompt and branch helpers"
```

---

## Task 3: glab List Wrappers

**Files:**
- Modify: `src/gitlab.rs`

- [ ] **Step 3.1: Add command-error classification tests**

Append these tests to `src/gitlab.rs`:

```rust
#[test]
fn glab_error_message_mentions_missing_glab() {
    let err = GitlabError::Command("glab not found".to_string());

    assert_eq!(err.to_string(), "glab not found");
}

#[test]
fn command_failure_uses_stderr_when_available() {
    let err = command_failure("issue list", "fatal: not authenticated\n");

    assert_eq!(err, GitlabError::Command("issue list: fatal: not authenticated".to_string()));
}

#[test]
fn command_failure_falls_back_to_generic_message() {
    let err = command_failure("mr list", "");

    assert_eq!(err, GitlabError::Command("mr list failed".to_string()));
}
```

- [ ] **Step 3.2: Run the new tests to verify failure**

Run:

```bash
cargo test gitlab::tests::command_failure_uses_stderr_when_available
```

Expected: fail because `command_failure` and `Display` are not implemented.

- [ ] **Step 3.3: Implement command wrappers**

Add these imports and implementations in `src/gitlab.rs`:

```rust
use std::fmt;
use std::path::Path;
use tokio::process::Command;

impl fmt::Display for GitlabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitlabError::Command(message) => f.write_str(message),
            GitlabError::Json(message) => write!(f, "could not parse glab JSON: {message}"),
            GitlabError::MissingField(field) => write!(f, "glab JSON missing field: {field}"),
        }
    }
}

pub fn command_failure(command: &str, stderr: &str) -> GitlabError {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        GitlabError::Command(format!("{command} failed"))
    } else {
        GitlabError::Command(format!("{command}: {trimmed}"))
    }
}

async fn run_glab_json(repo: &Path, args: &[&str], label: &str) -> Result<String, GitlabError> {
    let output = Command::new("glab")
        .current_dir(repo)
        .args(args)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                GitlabError::Command("glab not found".to_string())
            } else {
                GitlabError::Command(format!("{label}: {e}"))
            }
        })?;

    if !output.status.success() {
        return Err(command_failure(label, &String::from_utf8_lossy(&output.stderr)));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub async fn list_assigned_issues(repo: &Path) -> Result<Vec<GitlabIssue>, GitlabError> {
    let json = run_glab_json(
        repo,
        &["issue", "list", "--assignee=@me", "--output", "json", "--per-page", "30"],
        "issue list",
    )
    .await?;
    parse_issues(&json)
}

pub async fn list_review_merge_requests(
    repo: &Path,
) -> Result<Vec<GitlabMergeRequest>, GitlabError> {
    let json = run_glab_json(
        repo,
        &["mr", "list", "--reviewer=@me", "--output", "json", "--per-page", "30"],
        "mr list",
    )
    .await?;
    parse_merge_requests(&json)
}
```

- [ ] **Step 3.4: Run GitLab tests**

Run:

```bash
cargo test gitlab::tests
```

Expected: all `gitlab::tests` pass.

- [ ] **Step 3.5: Run full suite**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 3.6: Commit**

```bash
git add src/gitlab.rs
git commit -m "Add glab list wrappers"
```

---

## Task 4: MR Branch Preparation Without Checkout

**Files:**
- Modify: `src/agent.rs`

- [ ] **Step 4.1: Add pure refspec tests**

Append these tests to `src/agent.rs`'s existing test module:

```rust
#[test]
fn gitlab_mr_refspec_fetches_head_into_source_branch() {
    assert_eq!(
        gitlab_mr_refspec(184, "fix/remote-shell-profiles"),
        "merge-requests/184/head:fix/remote-shell-profiles"
    );
}

#[test]
fn gitlab_mr_refspec_preserves_slashes() {
    assert_eq!(
        gitlab_mr_refspec(7, "jona/gen-1102-detect-agents"),
        "merge-requests/7/head:jona/gen-1102-detect-agents"
    );
}
```

- [ ] **Step 4.2: Run the refspec test to verify failure**

Run:

```bash
cargo test agent::tests::gitlab_mr_refspec_fetches_head_into_source_branch
```

Expected: fail because `gitlab_mr_refspec` is not defined.

- [ ] **Step 4.3: Implement MR branch preparation helpers**

Add this near the existing async command wrappers in `src/agent.rs`, before `create_worktree`:

```rust
pub fn gitlab_mr_refspec(iid: u64, branch: &str) -> String {
    format!("merge-requests/{iid}/head:{branch}")
}

pub async fn local_branch_exists(repo_path: &Path, branch: &str) -> Result<bool, String> {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["rev-parse", "--verify", &format!("refs/heads/{branch}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|e| format!("git failed: {e}"))?;
    Ok(status.success())
}

pub async fn prepare_gitlab_mr_branch(
    repo_path: &Path,
    iid: u64,
    branch: &str,
) -> Result<(), String> {
    if local_branch_exists(repo_path, branch).await? {
        return Ok(());
    }

    let refspec = gitlab_mr_refspec(iid, branch);
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["fetch", "origin", &refspec])
        .output()
        .await
        .map_err(|e| format!("git failed: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "git fetch MR branch failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}
```

- [ ] **Step 4.4: Run agent tests**

Run:

```bash
cargo test agent::tests::gitlab_mr_refspec
```

Expected: both refspec tests pass.

- [ ] **Step 4.5: Run full suite**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 4.6: Commit**

```bash
git add src/agent.rs
git commit -m "Prepare GitLab MR branches without checkout"
```

---

## Task 5: Wizard State Types And Startup Behavior

**Files:**
- Modify: `src/app.rs:34-170`

- [ ] **Step 5.1: Add failing startup/source tests**

Append these tests to `src/app.rs`'s test module:

```rust
#[test]
fn start_new_agent_defaults_to_issue_source_and_fetches_issues() {
    let mut app = test_app();

    let cmds = app.update(Action::StartNewAgent);

    assert!(matches!(
        app.mode,
        Mode::NewAgent {
            source: NewAgentSource::Issue,
            focus: NewAgentFocus::Source,
            ..
        }
    ));
    assert!(cmds.iter().any(|c| matches!(c, Command::LoadBranches(_))));
    assert!(cmds.iter().any(|c| matches!(c, Command::LoadGitlabIssues(_))));
}

#[test]
fn source_picker_cycles_from_issue_to_mr_to_branch() {
    let mut app = test_app_in_new_agent_mode();
    if let Mode::NewAgent { source, focus, .. } = &mut app.mode {
        *source = NewAgentSource::Issue;
        *focus = NewAgentFocus::Source;
    }

    let cmds = app.update(Action::PickerNext);
    assert!(matches!(app.mode, Mode::NewAgent { source: NewAgentSource::Mr, .. }));
    assert!(cmds.iter().any(|c| matches!(c, Command::LoadGitlabMrs(_))));

    app.update(Action::PickerNext);
    assert!(matches!(app.mode, Mode::NewAgent { source: NewAgentSource::Branch, .. }));

    app.update(Action::PickerNext);
    assert!(matches!(app.mode, Mode::NewAgent { source: NewAgentSource::Issue, .. }));
}
```

- [ ] **Step 5.2: Run the startup test to verify failure**

Run:

```bash
cargo test app::tests::start_new_agent_defaults_to_issue_source_and_fetches_issues
```

Expected: fail because `NewAgentSource` and `LoadGitlabIssues` do not exist.

- [ ] **Step 5.3: Add source/list/prompt types**

In `src/app.rs`, add the import near the top:

```rust
use crate::gitlab::{GitlabIssue, GitlabMergeRequest};
```

Add these enums near `BranchMode` and `NewAgentFocus`:

```rust
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum NewAgentSource {
    Issue,
    Mr,
    Branch,
}

#[derive(Debug, PartialEq, Clone)]
pub enum RemoteList<T> {
    Idle,
    Loading,
    Loaded(Vec<T>),
    Failed(String),
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum PromptMode {
    Generated,
    Custom,
}
```

Extend `NewAgentFocus`:

```rust
#[derive(Debug, PartialEq, Clone)]
pub enum NewAgentFocus {
    Source,
    Agent,
    Repo,
    Search,
    SourceList,
    BranchToggle,
    BranchList,
    Name,
    Prompt,
}
```

Extend `Action`:

```rust
ResetPrompt,
GitlabIssuesLoaded(Result<Vec<GitlabIssue>, String>),
GitlabMrsLoaded(Result<Vec<GitlabMergeRequest>, String>),
```

Extend `Command`:

```rust
LoadGitlabIssues(PathBuf),
LoadGitlabMrs(PathBuf),
PrepareGitlabMrBranch {
    repo: PathBuf,
    mr_iid: u64,
    branch: String,
    session_name: String,
    agent_name: String,
    fresh_cmd: String,
},
```

Extend `Mode::NewAgent` fields:

```rust
source: NewAgentSource,
source_query: String,
source_index: usize,
issues: RemoteList<GitlabIssue>,
mrs: RemoteList<GitlabMergeRequest>,
selected_issue: Option<GitlabIssue>,
selected_mr: Option<GitlabMergeRequest>,
prompt_mode: PromptMode,
```

- [ ] **Step 5.4: Update StartNewAgent**

In `Action::StartNewAgent`, initialize the new fields and fetch issue data:

```rust
cmds.push(Command::LoadBranches(repos[0].clone()));
cmds.push(Command::LoadGitlabIssues(repos[0].clone()));
let today = chrono_free_date_str();
self.mode = Mode::NewAgent {
    source: NewAgentSource::Issue,
    repo_index: 0,
    branch_mode: BranchMode::New,
    prompt: String::new(),
    prompt_mode: PromptMode::Generated,
    focus: NewAgentFocus::Source,
    base_index: 0,
    branches: Vec::new(),
    existing_branches: Vec::new(),
    branch_name: format!("z-{today}-1"),
    name_pristine: true,
    agent_name: self.config.default_agent_name().to_string(),
    source_query: String::new(),
    source_index: 0,
    issues: RemoteList::Loading,
    mrs: RemoteList::Idle,
    selected_issue: None,
    selected_mr: None,
};
```

- [ ] **Step 5.5: Update test constructors**

Update every `Mode::NewAgent { ... }` test literal to include:

```rust
source: NewAgentSource::Branch,
source_query: String::new(),
source_index: 0,
issues: RemoteList::Idle,
mrs: RemoteList::Idle,
selected_issue: None,
selected_mr: None,
prompt_mode: PromptMode::Custom,
```

Use `PromptMode::Custom` in existing tests so current branch-flow prompt behavior remains unchanged while the new prompt-generation behavior gets separate dedicated tests.

- [ ] **Step 5.6: Implement source cycling in PickerNext/PickerPrev**

In both picker actions, when `focus == NewAgentFocus::Source`, cycle `source` and return a fetch command for issue/MR:

```rust
NewAgentFocus::Source => {
    *source = match source {
        NewAgentSource::Issue => NewAgentSource::Mr,
        NewAgentSource::Mr => NewAgentSource::Branch,
        NewAgentSource::Branch => NewAgentSource::Issue,
    };
    *source_index = 0;
    source_query.clear();
    match source {
        NewAgentSource::Issue => {
            *issues = RemoteList::Loading;
            reload_issues = true;
        }
        NewAgentSource::Mr => {
            *mrs = RemoteList::Loading;
            reload_mrs = true;
        }
        NewAgentSource::Branch => {}
    }
}
```

Add `reload_issues` and `reload_mrs` booleans next to `reload_branches`, then push `LoadGitlabIssues` / `LoadGitlabMrs` after the mutable borrow ends.

- [ ] **Step 5.7: Make focus cycling exhaustive for the new variants**

Replace the `Action::FocusNext` match for `Mode::NewAgent` with source-aware focus order:

```rust
*focus = match (&*focus, &*source, &*branch_mode) {
    (NewAgentFocus::Source, _, _) => NewAgentFocus::Agent,
    (NewAgentFocus::Agent, _, _) => NewAgentFocus::Repo,
    (NewAgentFocus::Repo, NewAgentSource::Issue | NewAgentSource::Mr, _) => NewAgentFocus::Search,
    (NewAgentFocus::Repo, NewAgentSource::Branch, _) => NewAgentFocus::BranchToggle,
    (NewAgentFocus::Search, _, _) => NewAgentFocus::SourceList,
    (NewAgentFocus::SourceList, _, _) => NewAgentFocus::Prompt,
    (NewAgentFocus::BranchToggle, _, _) => NewAgentFocus::BranchList,
    (NewAgentFocus::BranchList, NewAgentSource::Branch, BranchMode::New) => NewAgentFocus::Name,
    (NewAgentFocus::BranchList, _, _) => NewAgentFocus::Prompt,
    (NewAgentFocus::Name, _, _) => NewAgentFocus::Prompt,
    (NewAgentFocus::Prompt, _, _) => NewAgentFocus::Source,
};
```

Replace the `Action::FocusPrev` match with the reverse:

```rust
*focus = match (&*focus, &*source, &*branch_mode) {
    (NewAgentFocus::Source, _, _) => NewAgentFocus::Prompt,
    (NewAgentFocus::Agent, _, _) => NewAgentFocus::Source,
    (NewAgentFocus::Repo, _, _) => NewAgentFocus::Agent,
    (NewAgentFocus::Search, _, _) => NewAgentFocus::Repo,
    (NewAgentFocus::SourceList, _, _) => NewAgentFocus::Search,
    (NewAgentFocus::BranchToggle, _, _) => NewAgentFocus::Repo,
    (NewAgentFocus::BranchList, _, _) => NewAgentFocus::BranchToggle,
    (NewAgentFocus::Name, _, _) => NewAgentFocus::BranchList,
    (NewAgentFocus::Prompt, NewAgentSource::Issue | NewAgentSource::Mr, _) => NewAgentFocus::SourceList,
    (NewAgentFocus::Prompt, NewAgentSource::Branch, BranchMode::New) => NewAgentFocus::Name,
    (NewAgentFocus::Prompt, NewAgentSource::Branch, BranchMode::Existing) => NewAgentFocus::BranchList,
};
```

- [ ] **Step 5.8: Run focused state tests**

Run:

```bash
cargo test app::tests::start_new_agent_defaults_to_issue_source_and_fetches_issues app::tests::source_picker_cycles_from_issue_to_mr_to_branch
```

Expected: both tests pass.

- [ ] **Step 5.9: Run full suite**

Run:

```bash
cargo test
```

Expected: all tests pass after all test literals are updated.

- [ ] **Step 5.10: Commit**

```bash
git add src/app.rs
git commit -m "Add progressive wizard source state"
```

---

## Task 6: Fetch Result Handling And Local Filtering

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 6.1: Add failing fetch result tests**

Append these tests to `src/app.rs`:

```rust
fn issue(iid: u64, title: &str) -> GitlabIssue {
    GitlabIssue {
        iid,
        title: title.to_string(),
        description: None,
        web_url: None,
    }
}

fn mr(iid: u64, title: &str, source_branch: &str) -> GitlabMergeRequest {
    GitlabMergeRequest {
        iid,
        title: title.to_string(),
        description: None,
        web_url: None,
        source_branch: source_branch.to_string(),
        target_branch: Some("main".to_string()),
    }
}

#[test]
fn gitlab_issues_loaded_selects_first_issue_and_generates_prompt() {
    let mut app = test_app_in_new_agent_mode();
    if let Mode::NewAgent { source, prompt_mode, .. } = &mut app.mode {
        *source = NewAgentSource::Issue;
        *prompt_mode = PromptMode::Generated;
    }

    app.update(Action::GitlabIssuesLoaded(Ok(vec![issue(1102, "Detect agents remotely")])));

    if let Mode::NewAgent { issues, selected_issue, prompt, source_index, .. } = &app.mode {
        assert_eq!(*source_index, 0);
        assert!(matches!(issues, RemoteList::Loaded(v) if v.len() == 1));
        assert_eq!(selected_issue.as_ref().unwrap().iid, 1102);
        assert!(prompt.starts_with("Work on GitLab issue #1102"));
    } else {
        panic!("expected NewAgent mode");
    }
}

#[test]
fn gitlab_mrs_loaded_selects_first_mr_and_generates_prompt() {
    let mut app = test_app_in_new_agent_mode();
    if let Mode::NewAgent { source, prompt_mode, .. } = &mut app.mode {
        *source = NewAgentSource::Mr;
        *prompt_mode = PromptMode::Generated;
    }

    app.update(Action::GitlabMrsLoaded(Ok(vec![mr(184, "Use remote shell profiles", "fix/remote-shell-profiles")])));

    if let Mode::NewAgent { mrs, selected_mr, prompt, source_index, .. } = &app.mode {
        assert_eq!(*source_index, 0);
        assert!(matches!(mrs, RemoteList::Loaded(v) if v.len() == 1));
        assert_eq!(selected_mr.as_ref().unwrap().iid, 184);
        assert!(prompt.starts_with("Review GitLab MR !184"));
    } else {
        panic!("expected NewAgent mode");
    }
}

#[test]
fn gitlab_issue_failure_stays_in_wizard() {
    let mut app = test_app_in_new_agent_mode();

    app.update(Action::GitlabIssuesLoaded(Err("glab not found".to_string())));

    assert!(matches!(
        app.mode,
        Mode::NewAgent {
            issues: RemoteList::Failed(_),
            ..
        }
    ));
}

#[test]
fn custom_prompt_survives_issue_selection() {
    let mut app = test_app_in_new_agent_mode();
    if let Mode::NewAgent { source, prompt, prompt_mode, .. } = &mut app.mode {
        *source = NewAgentSource::Issue;
        *prompt = "my custom prompt".to_string();
        *prompt_mode = PromptMode::Custom;
    }

    app.update(Action::GitlabIssuesLoaded(Ok(vec![issue(1102, "Detect agents remotely")])));

    if let Mode::NewAgent { prompt, .. } = &app.mode {
        assert_eq!(prompt, "my custom prompt");
    } else {
        panic!("expected NewAgent mode");
    }
}
```

- [ ] **Step 6.2: Run one result test to verify failure**

Run:

```bash
cargo test app::tests::gitlab_issues_loaded_selects_first_issue_and_generates_prompt
```

Expected: fail because result handling is not implemented.

- [ ] **Step 6.3: Add filtered-list helpers**

Add pure helper functions in `src/app.rs` outside `impl App`:

```rust
fn matches_query(haystack: &str, query: &str) -> bool {
    haystack.to_ascii_lowercase().contains(&query.to_ascii_lowercase())
}

fn filtered_issue_indices(issues: &[GitlabIssue], query: &str) -> Vec<usize> {
    let trimmed = query.trim();
    issues
        .iter()
        .enumerate()
        .filter_map(|(i, issue)| {
            let label = format!("#{} {}", issue.iid, issue.title);
            (trimmed.is_empty() || matches_query(&label, trimmed)).then_some(i)
        })
        .collect()
}

fn filtered_mr_indices(mrs: &[GitlabMergeRequest], query: &str) -> Vec<usize> {
    let trimmed = query.trim();
    mrs.iter()
        .enumerate()
        .filter_map(|(i, mr)| {
            let label = format!("!{} {} {}", mr.iid, mr.title, mr.source_branch);
            (trimmed.is_empty() || matches_query(&label, trimmed)).then_some(i)
        })
        .collect()
}
```

- [ ] **Step 6.4: Handle `GitlabIssuesLoaded` and `GitlabMrsLoaded`**

Inside `App::update`, add result handling:

```rust
Action::GitlabIssuesLoaded(result) => {
    if let Mode::NewAgent {
        issues,
        selected_issue,
        source_index,
        prompt,
        prompt_mode,
        branches,
        branch_name,
        name_pristine,
        ..
    } = &mut self.mode {
        match result {
            Ok(items) => {
                let first = items.first().cloned();
                *issues = RemoteList::Loaded(items);
                *source_index = 0;
                *selected_issue = first.clone();
                if let Some(issue) = first {
                    if matches!(prompt_mode, PromptMode::Generated) {
                        *prompt = crate::gitlab::issue_prompt(&issue);
                    }
                    let today = chrono_free_date_str();
                    *branch_name = crate::gitlab::issue_branch_name(&issue, &today, branches);
                    *name_pristine = true;
                }
            }
            Err(error) => {
                *issues = RemoteList::Failed(error);
                *selected_issue = None;
            }
        }
    }
}
Action::GitlabMrsLoaded(result) => {
    if let Mode::NewAgent {
        mrs,
        selected_mr,
        source_index,
        prompt,
        prompt_mode,
        ..
    } = &mut self.mode {
        match result {
            Ok(items) => {
                let first = items.first().cloned();
                *mrs = RemoteList::Loaded(items);
                *source_index = 0;
                *selected_mr = first.clone();
                if let Some(mr) = first
                    && matches!(prompt_mode, PromptMode::Generated)
                {
                    *prompt = crate::gitlab::mr_prompt(&mr);
                }
            }
            Err(error) => {
                *mrs = RemoteList::Failed(error);
                *selected_mr = None;
            }
        }
    }
}
```

- [ ] **Step 6.5: Run fetch result tests**

Run:

```bash
cargo test app::tests::gitlab_issues_loaded app::tests::gitlab_mrs_loaded app::tests::gitlab_issue_failure_stays_in_wizard app::tests::custom_prompt_survives_issue_selection
```

Expected: matching tests pass.

- [ ] **Step 6.6: Run full suite**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 6.7: Commit**

```bash
git add src/app.rs
git commit -m "Handle GitLab wizard fetch results"
```

---

## Task 7: Prompt Editing And Source Picker Key Handling

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 7.1: Add failing key/prompt tests**

Append these tests to `src/app.rs`:

```rust
#[test]
fn typing_in_generated_prompt_marks_it_custom() {
    let mut app = test_app_in_new_agent_mode();
    if let Mode::NewAgent { focus, prompt, prompt_mode, .. } = &mut app.mode {
        *focus = NewAgentFocus::Prompt;
        *prompt = "generated".to_string();
        *prompt_mode = PromptMode::Generated;
    }

    app.update(Action::TypeChar('!'));

    if let Mode::NewAgent { prompt, prompt_mode, .. } = &app.mode {
        assert_eq!(prompt, "generated!");
        assert_eq!(*prompt_mode, PromptMode::Custom);
    } else {
        panic!("expected NewAgent mode");
    }
}

#[test]
fn reset_prompt_regenerates_selected_issue_prompt() {
    let mut app = test_app_in_new_agent_mode();
    let selected = issue(1102, "Detect agents remotely");
    if let Mode::NewAgent { source, selected_issue, prompt, prompt_mode, .. } = &mut app.mode {
        *source = NewAgentSource::Issue;
        *selected_issue = Some(selected);
        *prompt = "custom".to_string();
        *prompt_mode = PromptMode::Custom;
    }

    app.update(Action::ResetPrompt);

    if let Mode::NewAgent { prompt, prompt_mode, .. } = &app.mode {
        assert!(prompt.starts_with("Work on GitLab issue #1102"));
        assert_eq!(*prompt_mode, PromptMode::Generated);
    } else {
        panic!("expected NewAgent mode");
    }
}

#[test]
fn source_focus_left_right_maps_to_picker() {
    let mut app = test_app_in_new_agent_mode();
    if let Mode::NewAgent { focus, .. } = &mut app.mode {
        *focus = NewAgentFocus::Source;
    }

    assert!(matches!(app.handle_key(make_key(KeyCode::Right)), Some(Action::PickerNext)));
    assert!(matches!(app.handle_key(make_key(KeyCode::Left)), Some(Action::PickerPrev)));
}

#[test]
fn prompt_reset_key_only_maps_in_prompt_focus() {
    let mut app = test_app_in_new_agent_mode();
    if let Mode::NewAgent { focus, .. } = &mut app.mode {
        *focus = NewAgentFocus::Prompt;
    }

    assert!(matches!(app.handle_key(make_key(KeyCode::Char('r'))), Some(Action::ResetPrompt)));
}
```

- [ ] **Step 7.2: Run prompt test to verify failure**

Run:

```bash
cargo test app::tests::typing_in_generated_prompt_marks_it_custom
```

Expected: fail because prompt typing does not mark custom yet.

- [ ] **Step 7.3: Update text input actions**

In `Action::TypeChar` handling, when focus is `Prompt`, set `prompt_mode = PromptMode::Custom` before pushing the character:

```rust
NewAgentFocus::Prompt => {
    *prompt_mode = PromptMode::Custom;
    prompt.push(c);
}
```

In `Action::TypeBackspace`, when focus is `Prompt`, set custom before popping:

```rust
NewAgentFocus::Prompt => {
    *prompt_mode = PromptMode::Custom;
    prompt.pop();
}
```

- [ ] **Step 7.4: Implement `ResetPrompt`**

Add this update branch:

```rust
Action::ResetPrompt => {
    if let Mode::NewAgent {
        source,
        prompt,
        prompt_mode,
        selected_issue,
        selected_mr,
        ..
    } = &mut self.mode {
        match source {
            NewAgentSource::Issue => {
                if let Some(issue) = selected_issue.as_ref() {
                    *prompt = crate::gitlab::issue_prompt(issue);
                    *prompt_mode = PromptMode::Generated;
                }
            }
            NewAgentSource::Mr => {
                if let Some(mr) = selected_mr.as_ref() {
                    *prompt = crate::gitlab::mr_prompt(mr);
                    *prompt_mode = PromptMode::Generated;
                }
            }
            NewAgentSource::Branch => {
                prompt.clear();
                *prompt_mode = PromptMode::Generated;
            }
        }
    }
}
```

- [ ] **Step 7.5: Update key handling**

In `Mode::NewAgent` key handling:

```rust
KeyCode::Left if matches!(focus, NewAgentFocus::Source | NewAgentFocus::Agent | NewAgentFocus::Repo | NewAgentFocus::BranchToggle) => {
    Some(Action::PickerPrev)
}
KeyCode::Right if matches!(focus, NewAgentFocus::Source | NewAgentFocus::Agent | NewAgentFocus::Repo | NewAgentFocus::BranchToggle) => {
    Some(Action::PickerNext)
}
KeyCode::Char('r') if matches!(focus, NewAgentFocus::Prompt) => Some(Action::ResetPrompt),
```

- [ ] **Step 7.6: Run focused tests**

Run:

```bash
cargo test app::tests::typing_in_generated_prompt_marks_it_custom app::tests::reset_prompt_regenerates_selected_issue_prompt app::tests::source_focus_left_right_maps_to_picker app::tests::prompt_reset_key_only_maps_in_prompt_focus
```

Expected: all four tests pass.

- [ ] **Step 7.7: Run full suite**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 7.8: Commit**

```bash
git add src/app.rs
git commit -m "Add prompt editing behavior"
```

---

## Task 8: Create Commands For Issue And MR Sources

**Files:**
- Modify: `src/app.rs`
- Modify: `src/main.rs:315-430`

- [ ] **Step 8.1: Add failing create-command tests**

Append these tests to `src/app.rs`:

```rust
#[test]
fn picker_confirm_issue_creates_new_branch_command() {
    let mut app = test_app();
    app.mode = Mode::NewAgent {
        source: NewAgentSource::Issue,
        repo_index: 0,
        branch_mode: BranchMode::New,
        prompt: "issue prompt".to_string(),
        prompt_mode: PromptMode::Generated,
        focus: NewAgentFocus::Prompt,
        base_index: 0,
        branches: vec!["main".into()],
        existing_branches: Vec::new(),
        branch_name: "z-0505-1102-detect-agents-remotely".into(),
        name_pristine: true,
        agent_name: "codex".to_string(),
        source_query: String::new(),
        source_index: 0,
        issues: RemoteList::Loaded(vec![issue(1102, "Detect agents remotely")]),
        mrs: RemoteList::Idle,
        selected_issue: Some(issue(1102, "Detect agents remotely")),
        selected_mr: None,
    };

    let cmds = app.update(Action::PickerConfirm);

    assert!(matches!(app.mode, Mode::Normal));
    match &cmds[0] {
        Command::CreateAgent { branch, new_branch, base_branch, fresh_cmd, .. } => {
            assert_eq!(branch, "z-0505-1102-detect-agents-remotely");
            assert!(*new_branch);
            assert_eq!(base_branch.as_deref(), Some("main"));
            assert!(fresh_cmd.ends_with("'issue prompt'"));
        }
        other => panic!("expected CreateAgent, got {other:?}"),
    }
}

#[test]
fn picker_confirm_mr_prepares_mr_branch_command() {
    let selected_mr = mr(184, "Use remote shell profiles", "fix/remote-shell-profiles");
    let mut app = test_app();
    app.mode = Mode::NewAgent {
        source: NewAgentSource::Mr,
        repo_index: 0,
        branch_mode: BranchMode::Existing,
        prompt: "review prompt".to_string(),
        prompt_mode: PromptMode::Generated,
        focus: NewAgentFocus::Prompt,
        base_index: 0,
        branches: vec!["main".into()],
        existing_branches: Vec::new(),
        branch_name: String::new(),
        name_pristine: true,
        agent_name: "codex".to_string(),
        source_query: String::new(),
        source_index: 0,
        issues: RemoteList::Idle,
        mrs: RemoteList::Loaded(vec![selected_mr.clone()]),
        selected_issue: None,
        selected_mr: Some(selected_mr),
    };

    let cmds = app.update(Action::PickerConfirm);

    assert!(matches!(app.mode, Mode::Normal));
    match &cmds[0] {
        Command::PrepareGitlabMrBranch { mr_iid, branch, fresh_cmd, .. } => {
            assert_eq!(*mr_iid, 184);
            assert_eq!(branch, "fix/remote-shell-profiles");
            assert!(fresh_cmd.ends_with("'review prompt'"));
        }
        other => panic!("expected PrepareGitlabMrBranch, got {other:?}"),
    }
}
```

- [ ] **Step 8.2: Run MR create test to verify failure**

Run:

```bash
cargo test app::tests::picker_confirm_mr_prepares_mr_branch_command
```

Expected: fail because MR confirmation is not implemented.

- [ ] **Step 8.3: Refactor `PickerConfirm` to branch by source**

Inside `Action::PickerConfirm`, make the result include either `CreateAgent` data or MR prep data. Use this shape:

```rust
enum PendingCreate {
    Normal {
        repo: PathBuf,
        branch: String,
        new_branch: bool,
        base_branch: Option<String>,
        prompt: Option<String>,
        agent_name: String,
    },
    GitlabMr {
        repo: PathBuf,
        mr_iid: u64,
        branch: String,
        prompt: Option<String>,
        agent_name: String,
    },
}
```

For issue source, require `selected_issue.is_some()`, use `branch_name`, `new_branch = true`, and base from `branches[base_index]` with `"main"` fallback.

For MR source, require `selected_mr.is_some()`, use `selected_mr.iid` and `selected_mr.source_branch`, and emit `Command::PrepareGitlabMrBranch`.

For branch source, keep the current `BranchMode::New` / `BranchMode::Existing` behavior.

- [ ] **Step 8.4: Add creating agent entries for all sources**

Keep the existing optimistic `self.agents.push(Agent { status: AgentStatus::Creating, ... })` behavior. For MR source:

```rust
let sess_name = agent::session_name(&repo_name, &branch);
let slug = branch.replace('/', "-");
self.agents.push(Agent {
    repo_path: repo.clone(),
    repo_name,
    branch: branch.clone(),
    base_branch: None,
    worktree_path: PathBuf::new(),
    slug,
    session_name: sess_name.clone(),
    status: AgentStatus::Creating,
    agent_name: agent_name.clone(),
    last_pane_hash: None,
    last_attached_count: None,
    quiet_captures: 0,
    seen_activity_since_seed: false,
    was_spinner_visible: false,
    consecutive_emits: 0,
});
cmds.push(Command::PrepareGitlabMrBranch {
    repo,
    mr_iid,
    branch,
    session_name: sess_name,
    agent_name,
    fresh_cmd,
});
```

- [ ] **Step 8.5: Execute MR prep in `main.rs`**

In `execute`, add a `Command::PrepareGitlabMrBranch` match arm:

```rust
Command::PrepareGitlabMrBranch {
    repo,
    mr_iid,
    branch,
    session_name,
    agent_name,
    fresh_cmd,
} => {
    let tx = tx.clone();
    tokio::spawn(async move {
        if let Err(e) = agent::prepare_gitlab_mr_branch(&repo, mr_iid, &branch).await {
            let _ = tx.send(Action::AgentFailed { session: session_name, error: e });
            return;
        }
        match agent::create_worktree(&repo, &branch, false, None, &agent_name).await {
            Ok(worktree_path) => {
                if let Err(e) = agent::create_session(&session_name, &worktree_path, Some(&fresh_cmd)).await {
                    let _ = tx.send(Action::AgentFailed { session: session_name, error: e });
                    return;
                }
                let _ = tx.send(Action::AgentReady {
                    branch,
                    session: session_name,
                    worktree_path,
                });
            }
            Err(e) => {
                let _ = tx.send(Action::AgentFailed { session: session_name, error: e });
            }
        }
    });
}
```

- [ ] **Step 8.6: Run create-command tests**

Run:

```bash
cargo test app::tests::picker_confirm_issue_creates_new_branch_command app::tests::picker_confirm_mr_prepares_mr_branch_command
```

Expected: both tests pass.

- [ ] **Step 8.7: Run full suite**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 8.8: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "Create agents from GitLab sources"
```

---

## Task 9: Execute GitLab List Commands

**Files:**
- Modify: `src/main.rs:315-430`

- [ ] **Step 9.1: Add command execution arms**

In `execute`, add:

```rust
Command::LoadGitlabIssues(repo) => {
    let tx = tx.clone();
    tokio::spawn(async move {
        let result = crate::gitlab::list_assigned_issues(&repo)
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(Action::GitlabIssuesLoaded(result));
    });
}
Command::LoadGitlabMrs(repo) => {
    let tx = tx.clone();
    tokio::spawn(async move {
        let result = crate::gitlab::list_review_merge_requests(&repo)
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(Action::GitlabMrsLoaded(result));
    });
}
```

- [ ] **Step 9.2: Ensure repo changes refetch current source**

When `PickerNext` / `PickerPrev` changes `repo_index`, keep the existing branch reload and also fetch the active GitLab source:

```rust
if reload_branches {
    if let Some(cmd) = self.reload_branches_command() {
        cmds.push(cmd);
    }
}
if reload_issues {
    if let Some(repo) = self.current_new_agent_repo() {
        cmds.push(Command::LoadGitlabIssues(repo));
    }
}
if reload_mrs {
    if let Some(repo) = self.current_new_agent_repo() {
        cmds.push(Command::LoadGitlabMrs(repo));
    }
}
```

Add helper:

```rust
fn current_new_agent_repo(&self) -> Option<PathBuf> {
    if let Mode::NewAgent { repo_index, .. } = &self.mode {
        self.config.resolved_repos().get(*repo_index).cloned()
    } else {
        None
    }
}
```

- [ ] **Step 9.3: Run build**

Run:

```bash
cargo build
```

Expected: build succeeds.

- [ ] **Step 9.4: Run full suite**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 9.5: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "Wire GitLab fetch commands"
```

---

## Task 10: Progressive TUI Rendering

**Files:**
- Modify: `src/ui.rs:330-556`

- [ ] **Step 10.1: Add small rendering helper functions**

Inside `src/ui.rs`, before `draw_new_agent_modal`, add helpers:

```rust
fn source_label(source: crate::app::NewAgentSource) -> &'static str {
    match source {
        crate::app::NewAgentSource::Issue => "issue",
        crate::app::NewAgentSource::Mr => "mr",
        crate::app::NewAgentSource::Branch => "branch",
    }
}

fn remote_status_line<'a>(message: &'a str, label_w: u16) -> Line<'a> {
    Line::from(vec![
        Span::raw(" ".repeat(label_w as usize)),
        Span::styled(message, Style::default().fg(DIM)),
    ])
}
```

- [ ] **Step 10.2: Update modal destructuring**

In `draw_new_agent_modal`, include new fields:

```rust
let Mode::NewAgent {
    source,
    repo_index,
    branch_mode,
    prompt,
    prompt_mode,
    focus,
    base_index,
    branches,
    existing_branches,
    branch_name,
    name_pristine,
    agent_name,
    source_query,
    source_index,
    issues,
    mrs,
    selected_issue: _,
    selected_mr: _,
} = &app.mode else { return };
```

- [ ] **Step 10.3: Replace layout rows with progressive rows**

Use this vertical layout:

```rust
let source_list_height = match source {
    NewAgentSource::Issue => match issues {
        RemoteList::Loaded(items) => items.len().min(6).max(1) as u16,
        _ => 1,
    },
    NewAgentSource::Mr => match mrs {
        RemoteList::Loaded(items) => items.len().min(6).max(1) as u16,
        _ => 1,
    },
    NewAgentSource::Branch => active_list.len().min(6).max(1) as u16,
};
let show_branch_controls = matches!(source, NewAgentSource::Branch | NewAgentSource::Issue);
let show_branch_toggle = matches!(source, NewAgentSource::Branch);
let show_name = show_branch_controls
    && matches!(branch_mode, BranchMode::New)
    && !matches!(source, NewAgentSource::Issue);
let name_rows = if show_name { 2 } else { 0 };

let chunks = Layout::vertical([
    Constraint::Length(1),
    Constraint::Length(1),              // Source row
    Constraint::Length(1),
    Constraint::Length(1),              // Agent row
    Constraint::Length(1),
    Constraint::Length(1),              // Repo row
    Constraint::Length(1),
    Constraint::Length(if show_branch_toggle { 1 } else { 0 }),
    Constraint::Length(source_list_height),
    Constraint::Length(1),
    Constraint::Length(name_rows),
    Constraint::Length(1),
    Constraint::Min(3),
    Constraint::Length(1),
])
.split(inner);
```

- [ ] **Step 10.4: Render source row and source-specific list**

After `picker_row` is defined:

```rust
frame.render_widget(
    Paragraph::new(picker_row("Start from", source_label(*source), matches!(focus, NewAgentFocus::Source))),
    chunks[1],
);
frame.render_widget(
    Paragraph::new(picker_row("Agent", agent_name.as_str(), matches!(focus, NewAgentFocus::Agent))),
    chunks[3],
);
frame.render_widget(
    Paragraph::new(picker_row("Repo", repo_name, matches!(focus, NewAgentFocus::Repo))),
    chunks[5],
);
```

Render issue lists:

```rust
fn issue_lines<'a>(
    issues: &'a [crate::gitlab::GitlabIssue],
    query: &str,
    source_index: usize,
    label_w: u16,
) -> Vec<Line<'a>> {
    let query = query.to_ascii_lowercase();
    issues
        .iter()
        .enumerate()
        .filter(|issue| {
            let issue = issue.1;
            query.is_empty()
                || format!("#{} {}", issue.iid, issue.title)
                    .to_ascii_lowercase()
                    .contains(&query)
        })
        .map(|(actual_i, issue)| {
            let selected = actual_i == source_index;
            let indicator = if selected { "\u{2502} " } else { "  " };
            let style = if selected { Style::default().fg(TEXT) } else { Style::default().fg(DIM) };
            Line::from(vec![
                Span::raw(" ".repeat(label_w as usize)),
                Span::styled(indicator, style),
                Span::styled(format!("#{} {}", issue.iid, issue.title), style),
            ])
        })
        .collect()
}
```

Use the same shape for MRs with `!{iid} {title} {source_branch}`. If the list is `Loading`, render `loading assigned issues...` or `loading review MRs...`; if `Failed(error)`, render `error: {error}`; if empty, render `no assigned issues` or `no MRs needing review`.

- [ ] **Step 10.5: Render branch controls only when source is `Branch` or `Issue`**

For issue source, show branch name as a generated `Name` row but do not render the branch toggle. For branch source, keep the current branch toggle/list/name behavior.

For MR source, do not show branch name editing. The MR row already shows the source branch, and confirmation uses that branch directly.

- [ ] **Step 10.6: Update hint bar**

Use hints:

```rust
let hint_line = match focus {
    NewAgentFocus::Source | NewAgentFocus::Agent | NewAgentFocus::Repo | NewAgentFocus::BranchToggle => {
        footer_hint(&[("←/→", "cycle"), ("tab", "next"), ("q/esc", "cancel")])
    }
    NewAgentFocus::Search => {
        footer_hint(&[("type", "filter"), ("tab", "list"), ("esc", "cancel")])
    }
    NewAgentFocus::SourceList | NewAgentFocus::BranchList => {
        footer_hint(&[("↑/k", "up"), ("↓/j", "down"), ("enter", "start"), ("tab", "next")])
    }
    NewAgentFocus::Name => {
        footer_hint(&[("tab", "next"), ("esc", "cancel")])
    }
    NewAgentFocus::Prompt => {
        footer_hint(&[("enter", "start"), ("alt+enter", "newline"), ("r", "reset"), ("esc", "cancel")])
    }
};
```

- [ ] **Step 10.7: Build**

Run:

```bash
cargo build
```

Expected: build succeeds.

- [ ] **Step 10.8: Run tests**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 10.9: Manual visual check**

Run:

```bash
cargo run
```

Expected:
- `n` opens the wizard with `Start from issue`.
- Source row cycles through `issue`, `mr`, `branch`.
- Loading, empty, and error rows are dim and do not add new colors.
- Branch source still resembles the old flow.
- Prompt hints show reset only in prompt focus.

- [ ] **Step 10.10: Commit**

```bash
git add src/ui.rs
git commit -m "Render progressive GitLab wizard"
```

---

## Task 11: Search, Source List Navigation, And Focus Flow

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 11.1: Add failing search, focus, and source-list tests**

Append:

```rust
#[test]
fn focus_cycles_issue_mode_through_search_list_and_prompt() {
    let mut app = test_app_in_new_agent_mode();
    if let Mode::NewAgent { source, focus, .. } = &mut app.mode {
        *source = NewAgentSource::Issue;
        *focus = NewAgentFocus::Source;
    }

    let expected = vec![
        NewAgentFocus::Agent,
        NewAgentFocus::Repo,
        NewAgentFocus::Search,
        NewAgentFocus::SourceList,
        NewAgentFocus::Prompt,
        NewAgentFocus::Source,
    ];

    for exp in expected {
        app.update(Action::FocusNext);
        if let Mode::NewAgent { focus, .. } = &app.mode {
            assert_eq!(*focus, exp);
        } else {
            panic!("expected NewAgent mode");
        }
    }
}

#[test]
fn focus_cycles_branch_mode_through_existing_branch_controls() {
    let mut app = test_app_in_new_agent_mode();
    if let Mode::NewAgent { source, focus, .. } = &mut app.mode {
        *source = NewAgentSource::Branch;
        *focus = NewAgentFocus::Source;
    }

    let expected = vec![
        NewAgentFocus::Agent,
        NewAgentFocus::Repo,
        NewAgentFocus::BranchToggle,
        NewAgentFocus::BranchList,
        NewAgentFocus::Name,
        NewAgentFocus::Prompt,
        NewAgentFocus::Source,
    ];

    for exp in expected {
        app.update(Action::FocusNext);
        if let Mode::NewAgent { focus, .. } = &app.mode {
            assert_eq!(*focus, exp);
        } else {
            panic!("expected NewAgent mode");
        }
    }
}

#[test]
fn search_typing_filters_issues_and_selects_first_match() {
    let mut app = test_app_in_new_agent_mode();
    if let Mode::NewAgent {
        source,
        focus,
        issues,
        selected_issue,
        source_index,
        ..
    } = &mut app.mode {
        *source = NewAgentSource::Issue;
        *focus = NewAgentFocus::Search;
        *issues = RemoteList::Loaded(vec![issue(1, "Alpha"), issue(2, "Beta")]);
        *selected_issue = Some(issue(1, "Alpha"));
        *source_index = 0;
    }

    app.update(Action::TypeChar('b'));

    if let Mode::NewAgent { source_query, source_index, selected_issue, .. } = &app.mode {
        assert_eq!(source_query, "b");
        assert_eq!(*source_index, 1);
        assert_eq!(selected_issue.as_ref().unwrap().iid, 2);
    } else {
        panic!("expected NewAgent mode");
    }
}

#[test]
fn issue_source_list_navigation_updates_selected_issue() {
    let mut app = test_app_in_new_agent_mode();
    let first = issue(1, "First");
    let second = issue(2, "Second");
    if let Mode::NewAgent {
        source,
        focus,
        issues,
        selected_issue,
        source_index,
        ..
    } = &mut app.mode {
        *source = NewAgentSource::Issue;
        *focus = NewAgentFocus::SourceList;
        *issues = RemoteList::Loaded(vec![first, second.clone()]);
        *selected_issue = Some(issue(1, "First"));
        *source_index = 0;
    }

    app.update(Action::PickerNext);

    if let Mode::NewAgent { source_index, selected_issue, .. } = &app.mode {
        assert_eq!(*source_index, 1);
        assert_eq!(selected_issue.as_ref().unwrap().iid, 2);
    } else {
        panic!("expected NewAgent mode");
    }
}

#[test]
fn mr_source_list_navigation_updates_selected_mr() {
    let mut app = test_app_in_new_agent_mode();
    let first = mr(1, "First", "a");
    let second = mr(2, "Second", "b");
    if let Mode::NewAgent {
        source,
        focus,
        mrs,
        selected_mr,
        source_index,
        ..
    } = &mut app.mode {
        *source = NewAgentSource::Mr;
        *focus = NewAgentFocus::SourceList;
        *mrs = RemoteList::Loaded(vec![first, second.clone()]);
        *selected_mr = Some(mr(1, "First", "a"));
        *source_index = 0;
    }

    app.update(Action::PickerNext);

    if let Mode::NewAgent { source_index, selected_mr, .. } = &app.mode {
        assert_eq!(*source_index, 1);
        assert_eq!(selected_mr.as_ref().unwrap().iid, 2);
    } else {
        panic!("expected NewAgent mode");
    }
}
```

- [ ] **Step 11.2: Run search test to verify failure**

Run:

```bash
cargo test app::tests::search_typing_filters_issues_and_selects_first_match
```

Expected: fail because search typing is not wired.

- [ ] **Step 11.3: Add source selection helpers**

Add these helpers near `filtered_issue_indices` / `filtered_mr_indices`:

```rust
fn select_issue_by_index(
    issues: &[GitlabIssue],
    index: usize,
    prompt: &mut String,
    prompt_mode: PromptMode,
    branches: &[String],
    branch_name: &mut String,
    name_pristine: &mut bool,
) -> Option<GitlabIssue> {
    let issue = issues.get(index).cloned()?;
    if matches!(prompt_mode, PromptMode::Generated) {
        *prompt = crate::gitlab::issue_prompt(&issue);
    }
    let today = chrono_free_date_str();
    *branch_name = crate::gitlab::issue_branch_name(&issue, &today, branches);
    *name_pristine = true;
    Some(issue)
}

fn select_mr_by_index(
    mrs: &[GitlabMergeRequest],
    index: usize,
    prompt: &mut String,
    prompt_mode: PromptMode,
) -> Option<GitlabMergeRequest> {
    let mr = mrs.get(index).cloned()?;
    if matches!(prompt_mode, PromptMode::Generated) {
        *prompt = crate::gitlab::mr_prompt(&mr);
    }
    Some(mr)
}
```

- [ ] **Step 11.4: Implement search typing**

In `Action::TypeChar`, before prompt/name handling, add a `Search` branch:

```rust
NewAgentFocus::Search => {
    source_query.push(c);
    match source {
        NewAgentSource::Issue => {
            if let RemoteList::Loaded(items) = issues {
                if let Some(first) = filtered_issue_indices(items, source_query).first().copied() {
                    *source_index = first;
                    *selected_issue = select_issue_by_index(
                        items,
                        first,
                        prompt,
                        *prompt_mode,
                        branches,
                        branch_name,
                        name_pristine,
                    );
                }
            }
        }
        NewAgentSource::Mr => {
            if let RemoteList::Loaded(items) = mrs {
                if let Some(first) = filtered_mr_indices(items, source_query).first().copied() {
                    *source_index = first;
                    *selected_mr = select_mr_by_index(items, first, prompt, *prompt_mode);
                }
            }
        }
        NewAgentSource::Branch => {}
    }
}
```

In `Action::TypeBackspace`, add the same logic after `source_query.pop()`. If no filtered result exists, leave `selected_issue` or `selected_mr` unchanged and keep the current `source_index`; this avoids destructive selection loss while the user is typing a temporary unmatched query.

- [ ] **Step 11.5: Implement `SourceList` navigation**

In `PickerNext` / `PickerPrev`, add `NewAgentFocus::SourceList` handling:

```rust
NewAgentFocus::SourceList => {
    match source {
        NewAgentSource::Issue => {
            if let RemoteList::Loaded(items) = issues {
                let visible = filtered_issue_indices(items, source_query);
                if !visible.is_empty() {
                    let current_visible = visible.iter().position(|i| i == source_index).unwrap_or(0);
                    let next_visible = (current_visible + 1) % visible.len();
                    *source_index = visible[next_visible];
                    *selected_issue = select_issue_by_index(
                        items,
                        *source_index,
                        prompt,
                        *prompt_mode,
                        branches,
                        branch_name,
                        name_pristine,
                    );
                }
            }
        }
        NewAgentSource::Mr => {
            if let RemoteList::Loaded(items) = mrs {
                let visible = filtered_mr_indices(items, source_query);
                if !visible.is_empty() {
                    let current_visible = visible.iter().position(|i| i == source_index).unwrap_or(0);
                    let next_visible = (current_visible + 1) % visible.len();
                    *source_index = visible[next_visible];
                    *selected_mr = select_mr_by_index(items, *source_index, prompt, *prompt_mode);
                }
            }
        }
        NewAgentSource::Branch => {}
    }
}
```

Use the checked-sub equivalent for `PickerPrev`.

- [ ] **Step 11.6: Update key handling for Search and SourceList**

Map up/down/j/k to picker actions:

```rust
KeyCode::Up if matches!(focus, NewAgentFocus::SourceList | NewAgentFocus::BranchList) => Some(Action::PickerPrev),
KeyCode::Down if matches!(focus, NewAgentFocus::SourceList | NewAgentFocus::BranchList) => Some(Action::PickerNext),
KeyCode::Char('k') if matches!(focus, NewAgentFocus::SourceList | NewAgentFocus::BranchList) => Some(Action::PickerPrev),
KeyCode::Char('j') if matches!(focus, NewAgentFocus::SourceList | NewAgentFocus::BranchList) => Some(Action::PickerNext),
```

Map typing in search to text input:

```rust
KeyCode::Backspace if matches!(focus, NewAgentFocus::Search | NewAgentFocus::Prompt | NewAgentFocus::Name) => Some(Action::TypeBackspace),
KeyCode::Char(c) if matches!(focus, NewAgentFocus::Search | NewAgentFocus::Prompt | NewAgentFocus::Name) => Some(Action::TypeChar(c)),
```

- [ ] **Step 11.7: Run search and source-list tests**

Run:

```bash
cargo test app::tests::focus_cycles_issue_mode_through_search_list_and_prompt app::tests::focus_cycles_branch_mode_through_existing_branch_controls app::tests::search_typing_filters_issues_and_selects_first_match app::tests::issue_source_list_navigation_updates_selected_issue app::tests::mr_source_list_navigation_updates_selected_mr
```

Expected: all five tests pass.

- [ ] **Step 11.8: Run full suite**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 11.9: Commit**

```bash
git add src/app.rs
git commit -m "Navigate and search GitLab source lists"
```

---

## Task 12: Final Verification

**Files:**
- Verify all modified files.

- [ ] **Step 12.1: Format**

Run:

```bash
cargo fmt
```

Expected: command exits successfully.

- [ ] **Step 12.2: Test**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 12.3: Build**

Run:

```bash
cargo build
```

Expected: build succeeds.

- [ ] **Step 12.4: Manual GitLab issue flow**

From a configured GitLab repo with authenticated `glab`, run:

```bash
cargo run
```

Manual expected behavior:
- Press `n`.
- Wizard opens on `Start from issue`.
- Assigned issues load or a quiet inline error appears.
- Selecting an issue creates a branch named like `z-0505-1102-detect-agents-remotely`.
- Prompt contains the GitLab issue number, title, URL if available, and description if available.
- Pressing Enter creates the worktree/session and shows the agent row as creating/running.

- [ ] **Step 12.5: Manual GitLab MR flow**

In the same run:

```text
n -> Start from mr -> select MR -> enter
```

Manual expected behavior:
- MRs needing review load or a quiet inline error appears.
- MR row shows MR number/title/source branch.
- Confirming prepares the MR branch without switching the main worktree.
- Worktree/session uses the MR source branch.
- Prompt starts with `Review GitLab MR !<number>`.

- [ ] **Step 12.6: Manual branch regression**

In the same run:

```text
n -> Start from branch
```

Manual expected behavior:
- Existing new/existing branch flow still works.
- Empty custom prompt still creates an empty session.
- Agent/repo cycling still works.
- Delete/attach flows outside the wizard still work.

- [ ] **Step 12.7: Check diff**

Run:

```bash
git status --short
git diff --stat
```

Expected: only intended source/doc files are changed.

- [ ] **Step 12.8: Commit final formatting or verification-only adjustments**

If `cargo fmt` changed files:

```bash
git add src
git commit -m "Polish GitLab wizard implementation"
```

If no files changed, do not create an empty commit.
