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
        "--output",
        "json",
    ])
}

pub fn view_args(id_or_branch: &str) -> Vec<String> {
    strings(&["mr", "view", id_or_branch, "--output", "json"])
}

pub fn view_comments_args(id_or_branch: &str) -> Vec<String> {
    strings(&[
        "mr",
        "view",
        id_or_branch,
        "--comments",
        "--unresolved",
        "--output",
        "json",
    ])
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
}
