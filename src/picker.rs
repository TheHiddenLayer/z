use crate::gitlab::{GitlabIssue, GitlabMergeRequest};

fn matches_query(label: &str, query: &str) -> bool {
    let trimmed = query.trim();
    trimmed.is_empty()
        || label
            .to_ascii_lowercase()
            .contains(&trimmed.to_ascii_lowercase())
}

pub(crate) fn issue_label(issue: &GitlabIssue) -> String {
    format!("#{} {}", issue.iid, issue.title)
}

pub(crate) fn mr_label(mr: &GitlabMergeRequest) -> String {
    format!("!{} {} {}", mr.iid, mr.title, mr.source_branch)
}

pub(crate) fn filtered_issue_indices(issues: &[GitlabIssue], query: &str) -> Vec<usize> {
    issues
        .iter()
        .enumerate()
        .filter_map(|(index, issue)| matches_query(&issue_label(issue), query).then_some(index))
        .collect()
}

pub(crate) fn filtered_mr_indices(mrs: &[GitlabMergeRequest], query: &str) -> Vec<usize> {
    mrs.iter()
        .enumerate()
        .filter_map(|(index, mr)| matches_query(&mr_label(mr), query).then_some(index))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            target_branch: None,
        }
    }

    #[test]
    fn issue_labels_and_filters_use_the_same_text() {
        let issues = vec![issue(25, "Detect agents remotely"), issue(42, "Fix setup")];

        assert_eq!(issue_label(&issues[0]), "#25 Detect agents remotely");
        assert_eq!(filtered_issue_indices(&issues, "  AGENTS  "), vec![0]);
        assert_eq!(filtered_issue_indices(&issues, "#42"), vec![1]);
        assert_eq!(filtered_issue_indices(&issues, " "), vec![0, 1]);
    }

    #[test]
    fn mr_labels_and_filters_use_title_iid_and_branch() {
        let mrs = vec![
            mr(184, "Use remote shell profiles", "fix/remote-shell"),
            mr(205, "Document auth setup", "docs/auth"),
        ];

        assert_eq!(
            mr_label(&mrs[0]),
            "!184 Use remote shell profiles fix/remote-shell"
        );
        assert_eq!(filtered_mr_indices(&mrs, "SHELL"), vec![0]);
        assert_eq!(filtered_mr_indices(&mrs, "docs/auth"), vec![1]);
        assert_eq!(filtered_mr_indices(&mrs, "!184"), vec![0]);
    }
}
