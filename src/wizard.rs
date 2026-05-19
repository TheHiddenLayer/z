use crate::gitlab::{GitlabIssue, GitlabMergeRequest};
use crate::picker::{filtered_issue_indices, filtered_mr_indices};

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum BranchMode {
    New,
    Existing,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Source {
    Issue,
    Mr,
    Branch,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Remote<T> {
    Idle,
    Loading,
    Loaded(Vec<T>),
    Failed(String),
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Focus {
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

#[derive(Debug, PartialEq, Clone)]
pub struct NewAgent {
    pub repo_index: usize,
    pub source: Source,
    pub source_query: String,
    pub source_index: usize,
    pub issues: Remote<GitlabIssue>,
    pub mrs: Remote<GitlabMergeRequest>,
    pub selected_issue: Option<GitlabIssue>,
    pub selected_mr: Option<GitlabMergeRequest>,
    pub branch_mode: BranchMode,
    pub prompt: String,
    pub focus: Focus,
    pub base_index: usize,
    pub branches: Vec<String>,
    pub existing_branches: Vec<String>,
    pub branch_name: String,
    pub name_pristine: bool,
    pub agent_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Next,
    Prev,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PickerEffects {
    pub reload_branches: bool,
    pub load_issues: bool,
    pub load_mrs: bool,
}

pub fn generate_branch_name(branches: &[String], date_str: &str) -> String {
    let prefix = format!("z-{date_str}-");
    let max_n = branches
        .iter()
        .filter_map(|b| b.strip_prefix(&prefix))
        .filter_map(|suffix| suffix.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("{prefix}{}", max_n + 1)
}

impl NewAgent {
    pub fn new(today: &str, agent_name: String) -> Self {
        Self {
            repo_index: 0,
            source: Source::Branch,
            source_query: String::new(),
            source_index: 0,
            issues: Remote::Idle,
            mrs: Remote::Idle,
            selected_issue: None,
            selected_mr: None,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: Focus::Repo,
            base_index: 0,
            branches: Vec::new(),
            existing_branches: Vec::new(),
            branch_name: format!("z-{today}-1"),
            name_pristine: true,
            agent_name,
        }
    }

    pub fn select_issue(&mut self, index: usize, today: &str) -> Option<GitlabIssue> {
        let issue = match &self.issues {
            Remote::Loaded(items) => items.get(index)?.clone(),
            _ => return None,
        };
        self.source_index = index;
        self.selected_issue = Some(issue.clone());
        self.prompt = crate::gitlab::issue_prompt(&issue);
        self.branch_name = crate::gitlab::issue_branch_name(&issue, today, &self.branches);
        self.name_pristine = true;
        Some(issue)
    }

    pub fn select_mr(&mut self, index: usize) -> Option<GitlabMergeRequest> {
        let mr = match &self.mrs {
            Remote::Loaded(items) => items.get(index)?.clone(),
            _ => return None,
        };
        self.source_index = index;
        self.selected_mr = Some(mr.clone());
        self.prompt = crate::gitlab::mr_prompt(&mr);
        Some(mr)
    }

    pub fn prompt_for_source(&self) -> String {
        match self.source {
            Source::Issue => self
                .selected_issue
                .as_ref()
                .map(crate::gitlab::issue_prompt)
                .unwrap_or_default(),
            Source::Mr => self
                .selected_mr
                .as_ref()
                .map(crate::gitlab::mr_prompt)
                .unwrap_or_default(),
            Source::Branch => String::new(),
        }
    }

    pub fn move_picker(
        &mut self,
        direction: Direction,
        repo_count: usize,
        today: &str,
    ) -> PickerEffects {
        let mut effects = PickerEffects::default();
        match self.focus {
            Focus::Source => {
                self.source = match (direction, self.source) {
                    (Direction::Next, Source::Branch) => Source::Mr,
                    (Direction::Next, Source::Mr) => Source::Issue,
                    (Direction::Next, Source::Issue) => Source::Branch,
                    (Direction::Prev, Source::Branch) => Source::Issue,
                    (Direction::Prev, Source::Issue) => Source::Mr,
                    (Direction::Prev, Source::Mr) => Source::Branch,
                };
                self.source_index = 0;
                self.source_query.clear();
                match self.source {
                    Source::Issue => {
                        self.issues = Remote::Loading;
                        effects.load_issues = true;
                    }
                    Source::Mr => {
                        self.mrs = Remote::Loading;
                        effects.load_mrs = true;
                    }
                    Source::Branch => {}
                }
                self.prompt = self.prompt_for_source();
            }
            Focus::Repo if repo_count > 1 => {
                self.repo_index = match direction {
                    Direction::Next => (self.repo_index + 1) % repo_count,
                    Direction::Prev => self.repo_index.checked_sub(1).unwrap_or(repo_count - 1),
                };
                effects.reload_branches = true;
                match self.source {
                    Source::Issue => {
                        self.source_index = 0;
                        self.source_query.clear();
                        self.selected_issue = None;
                        self.issues = Remote::Loading;
                        effects.load_issues = true;
                    }
                    Source::Mr => {
                        self.source_index = 0;
                        self.source_query.clear();
                        self.selected_mr = None;
                        self.mrs = Remote::Loading;
                        effects.load_mrs = true;
                    }
                    Source::Branch => {}
                }
                self.prompt = self.prompt_for_source();
            }
            Focus::BranchToggle => {
                self.branch_mode = match self.branch_mode {
                    BranchMode::New => BranchMode::Existing,
                    BranchMode::Existing => BranchMode::New,
                };
                self.base_index = 0;
            }
            Focus::BranchList => {
                let count = match self.branch_mode {
                    BranchMode::New => self.branches.len(),
                    BranchMode::Existing => self.existing_branches.len(),
                };
                if count > 0 {
                    self.base_index = match direction {
                        Direction::Next => (self.base_index + 1) % count,
                        Direction::Prev => self.base_index.checked_sub(1).unwrap_or(count - 1),
                    };
                }
            }
            Focus::Search | Focus::SourceList => {
                self.select_relative_filtered(direction, today);
            }
            Focus::Agent | Focus::Name | Focus::Prompt | Focus::Repo => {}
        }
        effects
    }

    pub fn type_char(&mut self, c: char, today: &str) {
        match self.focus {
            Focus::Search => {
                self.source_query.push(c);
                self.select_first_filtered(today);
            }
            Focus::Name => {
                if self.name_pristine {
                    self.branch_name.clear();
                    self.name_pristine = false;
                }
                self.branch_name.push(c);
            }
            _ => {}
        }
    }

    pub fn backspace(&mut self, today: &str) {
        match self.focus {
            Focus::Search => {
                self.source_query.pop();
                self.select_first_filtered(today);
            }
            Focus::Name => {
                if self.name_pristine {
                    self.branch_name.clear();
                    self.name_pristine = false;
                } else {
                    self.branch_name.pop();
                }
            }
            _ => {}
        }
    }

    pub fn focus_next(&mut self, today: &str) {
        self.restore_empty_branch_name(today);
        self.focus = match (self.focus, self.source, self.branch_mode) {
            (Focus::Repo, _, _) => Focus::Source,
            (Focus::Source, Source::Issue | Source::Mr, _) => Focus::Search,
            (Focus::Source, Source::Branch, _) => Focus::BranchToggle,
            (Focus::Search, _, _) => Focus::SourceList,
            (Focus::SourceList, _, _) => Focus::Prompt,
            (Focus::BranchToggle, _, _) => Focus::BranchList,
            (Focus::BranchList, Source::Branch, BranchMode::New) => Focus::Name,
            (Focus::BranchList, _, _) => Focus::Prompt,
            (Focus::Name, _, _) => Focus::Prompt,
            (Focus::Prompt, _, _) => Focus::Agent,
            (Focus::Agent, _, _) => Focus::Repo,
        };
    }

    pub fn focus_prev(&mut self, today: &str) {
        self.restore_empty_branch_name(today);
        self.focus = match (self.focus, self.source, self.branch_mode) {
            (Focus::Repo, _, _) => Focus::Agent,
            (Focus::Source, _, _) => Focus::Repo,
            (Focus::Agent, _, _) => Focus::Prompt,
            (Focus::Search, _, _) => Focus::Source,
            (Focus::SourceList, _, _) => Focus::Search,
            (Focus::BranchToggle, _, _) => Focus::Source,
            (Focus::BranchList, _, _) => Focus::BranchToggle,
            (Focus::Name, _, _) => Focus::BranchList,
            (Focus::Prompt, Source::Issue | Source::Mr, _) => Focus::SourceList,
            (Focus::Prompt, Source::Branch, BranchMode::New) => Focus::Name,
            (Focus::Prompt, Source::Branch, BranchMode::Existing) => Focus::BranchList,
        };
    }

    fn restore_empty_branch_name(&mut self, today: &str) {
        if self.focus == Focus::Name && self.branch_name.is_empty() {
            self.branch_name = generate_branch_name(&self.branches, today);
            self.name_pristine = true;
        }
    }

    fn select_first_filtered(&mut self, today: &str) {
        let index = match self.source {
            Source::Issue => match &self.issues {
                Remote::Loaded(items) => filtered_issue_indices(items, &self.source_query)
                    .first()
                    .copied(),
                _ => None,
            },
            Source::Mr => match &self.mrs {
                Remote::Loaded(items) => filtered_mr_indices(items, &self.source_query)
                    .first()
                    .copied(),
                _ => None,
            },
            Source::Branch => None,
        };
        if let Some(index) = index {
            match self.source {
                Source::Issue => {
                    self.select_issue(index, today);
                }
                Source::Mr => {
                    self.select_mr(index);
                }
                Source::Branch => {}
            }
        }
    }

    fn select_relative_filtered(&mut self, direction: Direction, today: &str) {
        let index = match self.source {
            Source::Issue => match &self.issues {
                Remote::Loaded(items) => relative_index(
                    &filtered_issue_indices(items, &self.source_query),
                    self.source_index,
                    direction,
                ),
                _ => None,
            },
            Source::Mr => match &self.mrs {
                Remote::Loaded(items) => relative_index(
                    &filtered_mr_indices(items, &self.source_query),
                    self.source_index,
                    direction,
                ),
                _ => None,
            },
            Source::Branch => None,
        };
        if let Some(index) = index {
            match self.source {
                Source::Issue => {
                    self.select_issue(index, today);
                }
                Source::Mr => {
                    self.select_mr(index);
                }
                Source::Branch => {}
            }
        }
    }
}

fn relative_index(indices: &[usize], current: usize, direction: Direction) -> Option<usize> {
    if indices.is_empty() {
        return None;
    }
    let pos = indices.iter().position(|i| *i == current);
    Some(match (direction, pos) {
        (Direction::Next, Some(pos)) => indices[(pos + 1) % indices.len()],
        (Direction::Next, None) => indices[0],
        (Direction::Prev, Some(pos)) => indices[pos.checked_sub(1).unwrap_or(indices.len() - 1)],
        (Direction::Prev, None) => indices[indices.len() - 1],
    })
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
            target_branch: Some("main".to_string()),
        }
    }

    #[test]
    fn issue_selection_updates_prompt_and_branch_name() {
        let mut state = NewAgent::new("0509", "codex".to_string());
        state.branches = vec!["main".into()];
        state.issues = Remote::Loaded(vec![issue(42, "Fix setup flow")]);

        let selected = state.select_issue(0, "0509").unwrap();

        assert_eq!(selected.iid, 42);
        assert_eq!(state.source_index, 0);
        assert!(state.prompt.starts_with("Work on GitLab issue #42"));
        assert!(state.branch_name.contains("fix-setup-flow"));
        assert!(state.name_pristine);
    }

    #[test]
    fn branch_name_generation_increments_existing_day_suffixes() {
        let branches = vec![
            "main".to_string(),
            "z-0509-1".to_string(),
            "z-0509-2".to_string(),
            "z-0508-9".to_string(),
        ];

        assert_eq!(generate_branch_name(&branches, "0509"), "z-0509-3");
    }

    #[test]
    fn source_picker_cycles_branch_mr_issue_and_reports_load_effects() {
        let mut state = NewAgent::new("0509", "codex".to_string());
        state.source = Source::Branch;
        state.focus = Focus::Source;

        let effects = state.move_picker(Direction::Next, 1, "0509");

        assert_eq!(state.source, Source::Mr);
        assert_eq!(state.source_index, 0);
        assert!(effects.load_mrs);
        assert!(!effects.load_issues);
        assert!(!effects.reload_branches);

        let effects = state.move_picker(Direction::Next, 1, "0509");

        assert_eq!(state.source, Source::Issue);
        assert!(effects.load_issues);
        assert!(!effects.load_mrs);
        assert!(!effects.reload_branches);

        let effects = state.move_picker(Direction::Next, 1, "0509");

        assert_eq!(state.source, Source::Branch);
        assert!(!effects.load_issues);
        assert!(!effects.load_mrs);
        assert!(!effects.reload_branches);
    }

    #[test]
    fn search_input_selects_first_matching_mr() {
        let mut state = NewAgent::new("0509", "codex".to_string());
        state.source = Source::Mr;
        state.focus = Focus::Search;
        state.mrs = Remote::Loaded(vec![
            mr(1, "Docs update", "docs/update"),
            mr(2, "Auth fix", "feature/auth-fix"),
        ]);

        state.type_char('a', "0509");
        state.type_char('u', "0509");

        assert_eq!(state.source_query, "au");
        assert_eq!(state.source_index, 1);
        assert_eq!(state.selected_mr.as_ref().unwrap().iid, 2);
        assert!(state.prompt.starts_with("Review GitLab MR !2"));
    }

    #[test]
    fn branch_focus_skips_name_for_existing_branch_mode() {
        let mut state = NewAgent::new("0509", "codex".to_string());
        state.source = Source::Branch;
        state.branch_mode = BranchMode::Existing;
        state.focus = Focus::BranchList;

        state.focus_next("0509");

        assert_eq!(state.focus, Focus::Prompt);
    }

    #[test]
    fn leaving_empty_name_restores_generated_branch_name() {
        let mut state = NewAgent::new("0509", "codex".to_string());
        state.source = Source::Branch;
        state.focus = Focus::Name;
        state.branch_name.clear();
        state.name_pristine = false;
        state.branches = vec!["z-0509-1".into()];

        state.focus_next("0509");

        assert_eq!(state.branch_name, "z-0509-2");
        assert!(state.name_pristine);
        assert_eq!(state.focus, Focus::Prompt);
    }
}
