use std::fmt;
use std::future::Future;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;
use tokio::process::Command;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl fmt::Display for GitlabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitlabError::Command(message) => f.write_str(message),
            GitlabError::Json(message) => write!(f, "could not parse glab JSON: {message}"),
            GitlabError::MissingField(field) => write!(f, "glab JSON missing field: {field}"),
        }
    }
}

impl From<serde_json::Error> for GitlabError {
    fn from(value: serde_json::Error) -> Self {
        GitlabError::Json(value.to_string())
    }
}

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

const GITLAB_LIST_PAGE_SIZE: usize = 100;

fn review_merge_request_list_args(page: usize) -> Vec<String> {
    vec![
        "mr".to_string(),
        "list".to_string(),
        "--reviewer=@me".to_string(),
        "--output".to_string(),
        "json".to_string(),
        "--page".to_string(),
        page.to_string(),
        "--per-page".to_string(),
        GITLAB_LIST_PAGE_SIZE.to_string(),
    ]
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
        return Err(command_failure(
            label,
            &String::from_utf8_lossy(&output.stderr),
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub async fn list_assigned_issues(repo: &Path) -> Result<Vec<GitlabIssue>, GitlabError> {
    let json = run_glab_json(
        repo,
        &[
            "issue",
            "list",
            "--assignee=@me",
            "--output",
            "json",
            "--per-page",
            "30",
        ],
        "issue list",
    )
    .await?;
    parse_issues(&json)
}

async fn list_review_merge_requests_with_runner<F, Fut>(
    mut run_page: F,
) -> Result<Vec<GitlabMergeRequest>, GitlabError>
where
    F: FnMut(usize) -> Fut,
    Fut: Future<Output = Result<String, GitlabError>>,
{
    let mut page = 1;
    let mut all = Vec::new();
    loop {
        let json = run_page(page).await?;
        let mut items = parse_merge_requests(&json)?;
        let last_page = items.len() < GITLAB_LIST_PAGE_SIZE;
        all.append(&mut items);
        if last_page {
            return Ok(all);
        }
        page += 1;
    }
}

pub async fn list_review_merge_requests(
    repo: &Path,
) -> Result<Vec<GitlabMergeRequest>, GitlabError> {
    list_review_merge_requests_with_runner(|page| async move {
        let args = review_merge_request_list_args(page);
        let args = args.iter().map(String::as_str).collect::<Vec<_>>();
        run_glab_json(repo, &args, "mr list").await
    })
    .await
}

pub fn parse_mr_list(output: &str) -> Result<Option<MergeRequest>, String> {
    let value: Value =
        serde_json::from_str(output).map_err(|e| format!("json parse failed: {e}"))?;
    let Some(items) = value.as_array() else {
        return Err("json root is not an array".into());
    };
    Ok(items.first().map(parse_mr_value))
}

pub fn parse_mr_view(output: &str) -> Result<MergeRequest, String> {
    let value: Value =
        serde_json::from_str(output).map_err(|e| format!("json parse failed: {e}"))?;
    Ok(parse_mr_value(&value))
}

pub fn classify(mr: Option<&MergeRequest>) -> MrDisplay {
    let Some(mr) = mr else {
        return MrDisplay {
            glyph: " ",
            kind: MrDisplayKind::None,
        };
    };

    if matches!(mr.state, MrState::Merged) {
        return MrDisplay {
            glyph: "\u{2713}",
            kind: MrDisplayKind::Merged,
        };
    }
    if !matches!(mr.state, MrState::Open) {
        return MrDisplay {
            glyph: "!",
            kind: MrDisplayKind::Unknown,
        };
    }
    if mr.draft == Some(true) {
        return MrDisplay {
            glyph: "D",
            kind: MrDisplayKind::Draft,
        };
    }

    let merge = mr.merge_state.as_deref().unwrap_or("").to_ascii_lowercase();
    let pipe = mr
        .pipeline_state
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    let blocked = [
        "cannot_be_merged",
        "blocked",
        "conflict",
        "checking",
        "unchecked",
        "blocked_status",
        "ci_must_pass",
    ]
    .iter()
    .any(|needle| merge.contains(needle))
        || matches!(
            pipe.as_str(),
            "failed" | "canceled" | "cancelled" | "skipped"
        );
    if blocked {
        return MrDisplay {
            glyph: "B",
            kind: MrDisplayKind::Blocked,
        };
    }

    let ready_merge = matches!(merge.as_str(), "mergeable" | "can_be_merged");
    let ready_pipeline = pipe.is_empty() || matches!(pipe.as_str(), "success" | "passed");
    if ready_merge && ready_pipeline {
        return MrDisplay {
            glyph: "R",
            kind: MrDisplayKind::Ready,
        };
    }

    MrDisplay {
        glyph: "R",
        kind: MrDisplayKind::Open,
    }
}

fn parse_mr_value(v: &Value) -> MergeRequest {
    MergeRequest {
        source_branch: read_string_any(v, &["source_branch", "sourceBranch"]).unwrap_or_default(),
        target_branch: read_string_any(v, &["target_branch", "targetBranch"]),
        iid: read_u64_any(v, &["iid", "id"]),
        title: read_string_any(v, &["title"]),
        url: read_string_any(v, &["web_url", "webUrl", "url"]),
        state: parse_state(read_string_any(v, &["state"]).as_deref()),
        draft: read_bool_any(v, &["draft", "work_in_progress", "workInProgress"]),
        merge_state: read_string_any(
            v,
            &[
                "detailed_merge_status",
                "detailedMergeStatus",
                "merge_status",
                "mergeStatus",
                "merge_state",
                "mergeState",
            ],
        ),
        pipeline_state: read_pipeline_state(v),
        unresolved_count: read_u32_any(
            v,
            &[
                "unresolved_discussions_count",
                "unresolvedDiscussionsCount",
                "blocking_discussions_resolved",
                "user_notes_count",
            ],
        ),
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
        let Some(value) = v.get(*key) else {
            continue;
        };
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
        let Some(value) = v.get(*key) else {
            continue;
        };
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
        let Some(value) = v.get(*key) else {
            continue;
        };
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

pub fn list_args(source_branch: &str) -> Vec<String> {
    strings(&[
        "mr",
        "list",
        "--source-branch",
        source_branch,
        "--all",
        "--output",
        "json",
    ])
}

pub fn view_args(id_or_branch: &str) -> Vec<String> {
    strings(&["mr", "view", id_or_branch, "--output", "json"])
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

fn push_non_empty(parts: &mut Vec<String>, value: Option<&str>) {
    if let Some(value) = value {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }
}

pub fn issue_prompt(issue: &GitlabIssue) -> String {
    let mut parts = vec![format!(
        "Work on GitLab issue #{}: {}",
        issue.iid, issue.title
    )];
    push_non_empty(&mut parts, issue.web_url.as_deref());
    if let Some(description) = issue
        .description
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
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
        assert_eq!(
            mr.url.as_deref(),
            Some("https://gitlab.example.com/g/r/-/merge_requests/42")
        );
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
    fn classify_failed_pipeline_blocks_merge() {
        let mut mr = mr("feature/x");
        mr.merge_state = Some("mergeable".into());
        mr.pipeline_state = Some("failed".into());
        let d = classify(Some(&mr));
        assert_eq!(d.glyph, "B");
        assert_eq!(d.kind, MrDisplayKind::Blocked);
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
    fn classify_blocked_merge_state_is_b() {
        let mut mr = mr("feature/x");
        mr.merge_state = Some("blocked".into());
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

    #[test]
    fn list_args_are_argv_safe() {
        assert_eq!(
            list_args("feature/a b"),
            vec![
                "mr",
                "list",
                "--source-branch",
                "feature/a b",
                "--all",
                "--output",
                "json"
            ],
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

        assert_eq!(
            issues[0].web_url.as_deref(),
            Some("https://gitlab/x/y/-/issues/7")
        );
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
                web_url: Some(
                    "https://gitlab.example.com/acme/example/-/merge_requests/184".to_string()
                ),
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
        assert_eq!(
            mrs[0].web_url.as_deref(),
            Some("https://gitlab/x/y/-/merge_requests/9")
        );
    }

    #[tokio::test]
    async fn list_review_merge_requests_fetches_all_pages() {
        let seen_pages = std::cell::RefCell::new(Vec::new());

        let result = list_review_merge_requests_with_runner(|page| {
            seen_pages.borrow_mut().push(page);
            async move {
                let count = match page {
                    1 => 100,
                    2 => 2,
                    _ => panic!("unexpected page {page}"),
                };
                let items = (0..count)
                    .map(|offset| {
                        let iid = page as u64 * 1000 + offset as u64;
                        format!(
                            r#"{{"iid":{iid},"title":"MR {iid}","source_branch":"branch-{iid}"}}"#
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                Ok(format!("[{items}]"))
            }
        })
        .await
        .unwrap();

        assert_eq!(seen_pages.into_inner(), vec![1, 2]);
        assert_eq!(result.len(), 102);
        assert_eq!(result[0].iid, 1000);
        assert_eq!(result[101].iid, 2001);
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

    #[test]
    fn glab_error_message_mentions_missing_glab() {
        let err = GitlabError::Command("glab not found".to_string());

        assert_eq!(err.to_string(), "glab not found");
    }

    #[test]
    fn command_failure_uses_stderr_when_available() {
        let err = command_failure("issue list", "fatal: not authenticated\n");

        assert_eq!(
            err,
            GitlabError::Command("issue list: fatal: not authenticated".to_string())
        );
    }

    #[test]
    fn command_failure_falls_back_to_generic_message() {
        let err = command_failure("mr list", "");

        assert_eq!(err, GitlabError::Command("mr list failed".to_string()));
    }

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
            title: "a very long issue title that should not create a ridiculous branch name"
                .to_string(),
            description: None,
            web_url: None,
        };

        let branches =
            vec!["z-0505-1-a-very-long-issue-title-that-should-not-create-a-ridicu".to_string()];

        let branch = issue_branch_name(&issue, "0505", &branches);

        assert!(branch.len() <= 64, "branch was too long: {branch}");
        assert!(branch.starts_with("z-0505-1-a-very-long-issue-title"));
        assert!(branch.ends_with("-2"));
    }
}
