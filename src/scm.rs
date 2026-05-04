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

pub fn parse_gitlab_merge_requests(repo_name: &str, json: &str) -> ScmResult<Vec<MergeRequest>> {
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
        assert_eq!(
            mrs[0].web_url,
            "https://gitlab.example.com/acme/app/-/merge_requests/7"
        );
        assert_eq!(mrs[0].state, MergeRequestState::Draft);
    }

    #[test]
    fn malformed_json_returns_parse_failed() {
        let err = parse_gitlab_merge_requests("app", "not-json").unwrap_err();
        assert!(matches!(err, ScmError::ParseFailed(_)));
    }
}
