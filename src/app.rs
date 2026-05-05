use crate::agent::{self, Agent, AgentStatus};
use crate::config::Config;
use crate::gitlab::{MergeRequest, MrDisplayKind, classify};
use crate::notifications;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn chrono_free_date_str() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = now / 86400;
    let (mut y, mut m, mut d) = (2025u32, 1u32, 1u32);
    let mut remaining = (days as i64) - 20089;
    fn days_in_month(y: u32, m: u32) -> u32 {
        match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400)) {
                    29
                } else {
                    28
                }
            }
            _ => 30,
        }
    }
    while remaining >= days_in_month(y, m) as i64 {
        remaining -= days_in_month(y, m) as i64;
        m += 1;
        if m > 12 {
            m = 1;
            y += 1;
        }
    }
    d += remaining as u32;
    format!("{:02}{:02}", m, d)
}

// --- Action enum: every possible state transition ---

#[derive(Debug, Clone)]
pub enum Action {
    // Navigation
    MoveUp,
    MoveDown,

    // Mode transitions
    StartNewAgent,
    StartDelete,
    CancelMode,
    ToggleHelp,

    // New agent flow
    PickerNext,
    PickerPrev,
    PickerConfirm,
    TypeChar(char),
    TypeBackspace,
    FocusNext,
    FocusPrev,

    // Agent lifecycle (trigger async side effects)
    KillSession(String),
    DeleteAll {
        preserve_tmux: bool,
    },
    Attach,
    AttachReady(Agent),
    RefreshAgents,

    // Background results (pure state updates)
    AgentReady {
        branch: String,
        session: String,
        worktree_path: PathBuf,
    },
    AgentFailed {
        session: String,
        error: String,
    },
    DeleteFailed {
        branch: String,
        error: String,
    },
    AgentsRefreshed(Vec<Agent>),
    ActivityCaptured {
        session_name: String,
        /// Pane content from the same capture-pane call that produced
        /// `content_hash`. The selected agent uses this directly as preview;
        /// non-selected agents discard it. `None` only in tests that exercise
        /// activity hysteresis without caring about preview content.
        content: Option<String>,
        content_hash: u64,
        attached_count: u32,
    },
    BranchesLoaded {
        branches: Vec<String>,
    },
    TogglePreview,
    MrRefreshed {
        key: MrKey,
        snapshot: MrSnapshot,
    },
    MrCreate,
    MrOpen,
    MrMerge,
    MrMergeConfirmed,
    MrIntent(MrIntent),

    // System
    Tick,
    TerminalFocus(bool),
    Quit,
}

#[derive(Debug, PartialEq, Clone)]
pub enum BranchMode {
    New,
    Existing,
}

#[derive(Debug, PartialEq, Clone)]
pub enum NewAgentFocus {
    Agent,
    Repo,
    BranchToggle,
    BranchList,
    Name,
    Prompt,
}

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

fn generate_branch_name(branches: &[String], date_str: &str) -> String {
    let prefix = format!("z-{date_str}-");
    let max_n = branches
        .iter()
        .filter_map(|b| b.strip_prefix(&prefix))
        .filter_map(|suffix| suffix.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("{prefix}{}", max_n + 1)
}

// --- Command enum: side effects returned by update() ---

#[derive(Debug)]
pub enum Command {
    Discover(Vec<PathBuf>),
    LoadBranches(PathBuf),
    CaptureActivity {
        session_name: String,
    },
    CreateAgent {
        repo: PathBuf,
        branch: String,
        new_branch: bool,
        base_branch: Option<String>,
        session_name: String,
        agent_name: String,
        fresh_cmd: String,
    },
    KillSession(String),
    DeleteAgent {
        session_name: String,
        kill_session: bool,
        repo_path: PathBuf,
        worktree_path: PathBuf,
        branch: String,
    },
    Attach(Agent),
    PrepareAttach {
        agent: Agent,
        resume_cmd: String,
    },
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
        key: MrKey,
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
}

// --- Mode ---

#[derive(Debug, PartialEq)]
pub enum Mode {
    Normal,
    NewAgent {
        repo_index: usize,
        branch_mode: BranchMode,
        prompt: String,
        focus: NewAgentFocus,
        base_index: usize,
        branches: Vec<String>,
        existing_branches: Vec<String>,
        branch_name: String,
        name_pristine: bool,
        agent_name: String,
    },
    ConfirmDelete,
    ConfirmMerge,
}

fn find_main_branch(branches: &[String]) -> usize {
    branches
        .iter()
        .position(|b| b == "main")
        .or_else(|| branches.iter().position(|b| b == "master"))
        .unwrap_or(0)
}

// --- App ---

pub struct App {
    pub agents: Vec<Agent>,
    pub selected: usize,
    pub mode: Mode,
    pub config: Config,
    pub should_quit: bool,
    pub status_message: Option<String>,
    pub preview_content: Option<String>,
    pub spinner_frame: usize,
    pub dirty: bool,
    pub help_visible: bool,
    pub preview_mode: PreviewMode,
    pub mr_snapshots: HashMap<MrKey, MrSnapshot>,

    // Backpressure: prevent spawning new work when previous is in-flight
    discover_pending: bool,
    mr_refresh_pending: bool,
    mr_refresh_outstanding: usize,

    // Notification gating
    pub focused: bool,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self {
            agents: Vec::new(),
            selected: 0,
            mode: Mode::Normal,
            config,
            should_quit: false,
            status_message: None,
            preview_content: None,
            spinner_frame: 0,
            dirty: true, // render on first frame
            help_visible: false,
            preview_mode: PreviewMode::Terminal,
            mr_snapshots: HashMap::new(),
            discover_pending: false,
            mr_refresh_pending: false,
            mr_refresh_outstanding: 0,
            focused: true,
        }
    }

    fn should_notify(&self) -> bool {
        if !self.config.notifications.enabled {
            return false;
        }
        if self.config.notifications.only_when_unfocused && self.focused {
            return false;
        }
        true
    }

    /// Reseeds pane state after a tmux detach so the next capture's hash
    /// (which may differ purely due to pane reflow) doesn't masquerade as
    /// real activity. The hysteresis itself needs no special handling on
    /// detach: paused captures stall the quiet counter rather than expiring
    /// any deadline, so an event-loop gap can't synthesize a spurious
    /// "agent finished" edge.
    pub fn on_session_detached(&mut self, session_name: &str) {
        if let Some(agent) = self
            .agents
            .iter_mut()
            .find(|a| a.session_name == session_name)
        {
            agent.last_pane_hash = None;
            agent.quiet_captures = 0;
            agent.consecutive_emits = 0;
        }
    }

    pub fn selected_agent(&self) -> Option<&Agent> {
        self.agents.get(self.selected)
    }

    /// Activity capture for the selected session, used to repaint the preview
    /// pane immediately after navigation. The same Command type that the Tick
    /// loop fires periodically — its content lands in `preview_content` when
    /// the session matches the current selection.
    fn capture_selected_command(&self) -> Option<Command> {
        let agent = self.selected_agent()?;
        if agent.status.has_session() {
            Some(Command::CaptureActivity {
                session_name: agent.session_name.clone(),
            })
        } else {
            None
        }
    }

    fn reload_branches_command(&self) -> Option<Command> {
        if let Mode::NewAgent { repo_index, .. } = &self.mode {
            let repos = self.config.resolved_repos();
            repos
                .get(*repo_index)
                .map(|repo| Command::LoadBranches(repo.clone()))
        } else {
            None
        }
    }

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
        Some(
            mr.iid
                .map(|iid| iid.to_string())
                .unwrap_or_else(|| mr.source_branch.clone()),
        )
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

    fn schedule_mr_refresh(&mut self) -> Vec<Command> {
        if self.mr_refresh_pending {
            return Vec::new();
        }
        let cmds = self.refresh_mr_commands();
        if cmds.is_empty() {
            self.mr_refresh_pending = false;
            self.mr_refresh_outstanding = 0;
            return cmds;
        }
        self.mr_refresh_pending = true;
        self.mr_refresh_outstanding = cmds.len();
        cmds
    }

    fn selected_agent_fresh_cmd(&self, prompt: &str) -> Option<String> {
        let agent = self.selected_agent()?;
        self.config
            .fresh(&agent.agent_name, Some(prompt))
            .or_else(|| {
                self.config
                    .fresh(self.config.default_agent_name(), Some(prompt))
            })
    }

    fn selected_base_branch(&self) -> String {
        self.selected_agent()
            .and_then(|a| a.base_branch.clone())
            .or_else(|| self.selected_mr().and_then(|mr| mr.target_branch.clone()))
            .unwrap_or_else(|| "main".to_string())
    }

    /// Core state machine. Returns Commands for side effects to be executed by the caller.
    pub fn update(&mut self, action: Action) -> Vec<Command> {
        let mut cmds = vec![];
        // ActivityCaptured sets dirty itself only when something visible
        // actually changed. All other non-Tick actions change visible state
        // and need a redraw.
        match &action {
            Action::Tick | Action::ActivityCaptured { .. } | Action::TerminalFocus(_) => {}
            _ => {
                self.dirty = true;
            }
        }
        match action {
            // --- Navigation ---
            Action::MoveUp => {
                if self.selected > 0 {
                    self.selected -= 1;
                    // Honest blank > stale content: clear immediately and
                    // wait for the new selection's capture to land.
                    self.preview_content = None;
                    if let Some(cmd) = self.capture_selected_command() {
                        cmds.push(cmd);
                    }
                }
            }
            Action::MoveDown => {
                if self.selected + 1 < self.agents.len() {
                    self.selected += 1;
                    self.preview_content = None;
                    if let Some(cmd) = self.capture_selected_command() {
                        cmds.push(cmd);
                    }
                }
            }

            // --- Mode transitions ---
            Action::StartNewAgent => {
                let repos = self.config.resolved_repos();
                if repos.is_empty() {
                    self.status_message = Some("No repos configured".into());
                    return cmds;
                }
                cmds.push(Command::LoadBranches(repos[0].clone()));
                let today = chrono_free_date_str();
                self.mode = Mode::NewAgent {
                    repo_index: 0,
                    branch_mode: BranchMode::New,
                    prompt: String::new(),
                    focus: NewAgentFocus::Agent,
                    base_index: 0,
                    branches: Vec::new(),
                    existing_branches: Vec::new(),
                    branch_name: format!("z-{today}-1"),
                    name_pristine: true,
                    agent_name: self.config.default_agent_name().to_string(),
                };
            }
            Action::StartDelete => {
                if self.selected_agent().is_some() {
                    self.mode = Mode::ConfirmDelete;
                }
            }
            Action::CancelMode => {
                self.mode = Mode::Normal;
            }
            Action::ToggleHelp => {
                self.help_visible = !self.help_visible;
            }

            // --- Pickers ---
            Action::PickerNext => {
                let mut reload_branches = false;
                let repo_count = self.config.resolved_repos().len();
                let next_agent_name: Option<String> = if let Mode::NewAgent {
                    focus: NewAgentFocus::Agent,
                    agent_name,
                    ..
                } = &self.mode
                {
                    Some(self.config.cycle_next(agent_name).to_string())
                } else {
                    None
                };
                if let Mode::NewAgent {
                    focus,
                    repo_index,
                    base_index,
                    branches,
                    branch_mode,
                    existing_branches,
                    agent_name,
                    ..
                } = &mut self.mode
                {
                    match focus {
                        NewAgentFocus::Agent => {
                            if let Some(n) = next_agent_name {
                                *agent_name = n;
                            }
                        }
                        NewAgentFocus::Repo if repo_count > 1 => {
                            *repo_index = (*repo_index + 1) % repo_count;
                            reload_branches = true;
                        }
                        NewAgentFocus::Repo => {}
                        NewAgentFocus::BranchToggle => {
                            *branch_mode = match branch_mode {
                                BranchMode::New => BranchMode::Existing,
                                BranchMode::Existing => BranchMode::New,
                            };
                            *base_index = 0;
                        }
                        NewAgentFocus::BranchList => {
                            let count = match branch_mode {
                                BranchMode::New => branches.len(),
                                BranchMode::Existing => existing_branches.len(),
                            };
                            if count > 0 {
                                *base_index = (*base_index + 1) % count;
                            }
                        }
                        _ => {}
                    }
                }
                if reload_branches && let Some(cmd) = self.reload_branches_command() {
                    cmds.push(cmd);
                }
            }
            Action::PickerPrev => {
                let mut reload_branches = false;
                let repo_count = self.config.resolved_repos().len();
                let prev_agent_name: Option<String> = if let Mode::NewAgent {
                    focus: NewAgentFocus::Agent,
                    agent_name,
                    ..
                } = &self.mode
                {
                    Some(self.config.cycle_prev(agent_name).to_string())
                } else {
                    None
                };
                if let Mode::NewAgent {
                    focus,
                    repo_index,
                    base_index,
                    branches,
                    branch_mode,
                    existing_branches,
                    agent_name,
                    ..
                } = &mut self.mode
                {
                    match focus {
                        NewAgentFocus::Agent => {
                            if let Some(n) = prev_agent_name {
                                *agent_name = n;
                            }
                        }
                        NewAgentFocus::Repo if repo_count > 1 => {
                            *repo_index = repo_index.checked_sub(1).unwrap_or(repo_count - 1);
                            reload_branches = true;
                        }
                        NewAgentFocus::Repo => {}
                        NewAgentFocus::BranchToggle => {
                            *branch_mode = match branch_mode {
                                BranchMode::New => BranchMode::Existing,
                                BranchMode::Existing => BranchMode::New,
                            };
                            *base_index = 0;
                        }
                        NewAgentFocus::BranchList => {
                            let count = match branch_mode {
                                BranchMode::New => branches.len(),
                                BranchMode::Existing => existing_branches.len(),
                            };
                            if count > 0 {
                                *base_index = base_index.checked_sub(1).unwrap_or(count - 1);
                            }
                        }
                        _ => {}
                    }
                }
                if reload_branches && let Some(cmd) = self.reload_branches_command() {
                    cmds.push(cmd);
                }
            }
            Action::PickerConfirm => {
                let result = match &self.mode {
                    Mode::NewAgent {
                        repo_index,
                        branch_mode,
                        prompt,
                        base_index,
                        branches,
                        existing_branches,
                        branch_name,
                        agent_name,
                        ..
                    } => {
                        let repos = self.config.resolved_repos();
                        let repo = repos[*repo_index % repos.len()].clone();
                        let prompt_opt = if prompt.is_empty() {
                            None
                        } else {
                            Some(prompt.clone())
                        };
                        let name = agent_name.clone();

                        match branch_mode {
                            BranchMode::New => {
                                let base = branches
                                    .get(*base_index)
                                    .cloned()
                                    .unwrap_or_else(|| "main".to_string());
                                let branch_label = if branch_name.is_empty() {
                                    let today = chrono_free_date_str();
                                    generate_branch_name(branches, &today)
                                } else {
                                    branch_name.clone()
                                };
                                Some((repo, branch_label, true, Some(base), prompt_opt, name))
                            }
                            BranchMode::Existing => {
                                existing_branches.get(*base_index).map(|selected| {
                                    (repo, selected.clone(), false, None, prompt_opt, name)
                                })
                            }
                        }
                    }
                    _ => None,
                };

                if let Some((repo, branch, new_branch, base_branch, prompt_opt, agent_name)) =
                    result
                {
                    let repo_name = repo
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let sess_name = agent::session_name(&repo_name, &branch);
                    let slug = branch.replace('/', "-");
                    let fresh_cmd = self
                        .config
                        .fresh(&agent_name, prompt_opt.as_deref())
                        .expect("wizard agent_name is always in config");

                    self.agents.push(Agent {
                        repo_path: repo.clone(),
                        repo_name,
                        branch: branch.clone(),
                        base_branch: base_branch.clone(),
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

                    cmds.push(Command::CreateAgent {
                        repo,
                        branch,
                        new_branch,
                        base_branch,
                        session_name: sess_name,
                        agent_name,
                        fresh_cmd,
                    });
                    self.mode = Mode::Normal;
                } else if matches!(
                    self.mode,
                    Mode::NewAgent {
                        branch_mode: BranchMode::Existing,
                        ..
                    }
                ) {
                    self.status_message = Some("No existing branches available".into());
                }
            }

            // --- Text input ---
            Action::TypeChar(c) => {
                if let Mode::NewAgent {
                    focus,
                    prompt,
                    branch_name,
                    name_pristine,
                    ..
                } = &mut self.mode
                {
                    match focus {
                        NewAgentFocus::Prompt => prompt.push(c),
                        NewAgentFocus::Name => {
                            if *name_pristine {
                                branch_name.clear();
                                *name_pristine = false;
                            }
                            branch_name.push(c);
                        }
                        _ => {}
                    }
                }
            }
            Action::TypeBackspace => {
                if let Mode::NewAgent {
                    focus,
                    prompt,
                    branch_name,
                    name_pristine,
                    ..
                } = &mut self.mode
                {
                    match focus {
                        NewAgentFocus::Prompt => {
                            prompt.pop();
                        }
                        NewAgentFocus::Name => {
                            if *name_pristine {
                                branch_name.clear();
                                *name_pristine = false;
                            } else {
                                branch_name.pop();
                            }
                        }
                        _ => {}
                    }
                }
            }

            // --- Agent lifecycle ---
            Action::KillSession(name) => {
                cmds.push(Command::KillSession(name));
            }
            Action::DeleteAll { preserve_tmux } => {
                if let Some(agent) = self.selected_agent().cloned() {
                    self.status_message = Some(format!("Removing: {}", agent.branch));
                    cmds.push(Command::DeleteAgent {
                        session_name: agent.session_name,
                        kill_session: agent.status.has_session() && !preserve_tmux,
                        repo_path: agent.repo_path,
                        worktree_path: agent.worktree_path,
                        branch: agent.branch,
                    });
                }
                self.mode = Mode::Normal;
            }
            Action::Attach => {
                if let Some(agent) = self.selected_agent().cloned() {
                    if agent.status.has_session() {
                        cmds.push(Command::Attach(agent));
                    } else {
                        // Spawn session creation off the main loop so the
                        // UI stays responsive while tmux warms up. Resolve
                        // the resume command here so the spawn doesn't need
                        // access to Config; if the stored agent name isn't
                        // in the current config, fall back to the default
                        // and surface a non-fatal warning.
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
                        if self.status_message.is_none() {
                            self.status_message = Some(format!("Starting: {}", agent.branch));
                        }
                        cmds.push(Command::PrepareAttach { agent, resume_cmd });
                    }
                }
            }
            Action::AttachReady(agent) => {
                cmds.push(Command::Attach(agent));
            }
            Action::RefreshAgents => {
                self.discover_pending = true;
                cmds.push(Command::Discover(self.config.resolved_repos()));
            }

            // --- Background results ---
            Action::AgentReady {
                branch,
                session,
                worktree_path,
            } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.session_name == session) {
                    agent.status = AgentStatus::Running;
                    // Populate the path now so an immediate delete works
                    // without waiting for the next AgentsRefreshed cycle.
                    agent.worktree_path = worktree_path;
                }
                self.status_message = Some(format!("Ready: {}", branch));
            }
            Action::AgentFailed { session, error } => {
                let label = self
                    .agents
                    .iter()
                    .find(|a| a.session_name == session)
                    .map(|a| format!("{}/{}", a.repo_name, a.slug))
                    .unwrap_or_else(|| session.clone());
                if let Some(agent) = self.agents.iter_mut().find(|a| a.session_name == session) {
                    agent.status = AgentStatus::Error(error.clone());
                }
                if self.should_notify() {
                    notifications::fire(&format!("{label} failed"), &error);
                }
                self.status_message = Some(format!("Failed: {}", error));
            }
            Action::DeleteFailed { branch, error } => {
                self.status_message = Some(format!("Delete {branch}: {error}"));
            }
            Action::AgentsRefreshed(mut new_agents) => {
                self.discover_pending = false;
                // Carry over base_branch from existing agents (discover doesn't know it).
                // Also carry observation fields so shows_spinner() stays
                // continuous across the 3s refresh — discover re-seeds these to
                // defaults on every cycle.
                for new_agent in &mut new_agents {
                    if let Some(old) = self
                        .agents
                        .iter()
                        .find(|a| a.session_name == new_agent.session_name)
                    {
                        if new_agent.base_branch.is_none() {
                            new_agent.base_branch = old.base_branch.clone();
                        }
                        new_agent.last_pane_hash = old.last_pane_hash;
                        new_agent.last_attached_count = old.last_attached_count;
                        new_agent.quiet_captures = old.quiet_captures;
                        new_agent.seen_activity_since_seed = old.seen_activity_since_seed;
                        new_agent.was_spinner_visible = old.was_spinner_visible;
                        new_agent.consecutive_emits = old.consecutive_emits;
                    }
                }
                let creating: Vec<_> = self
                    .agents
                    .iter()
                    .filter(|a| matches!(a.status, AgentStatus::Creating))
                    .cloned()
                    .collect();
                for ca in creating {
                    if !new_agents.iter().any(|a| a.session_name == ca.session_name) {
                        new_agents.push(ca);
                    }
                }
                self.agents = new_agents;
                cmds.extend(self.schedule_mr_refresh());
                if self.selected >= self.agents.len() && !self.agents.is_empty() {
                    self.selected = self.agents.len() - 1;
                }
            }
            Action::ActivityCaptured {
                session_name,
                content,
                content_hash,
                attached_count,
            } => {
                // If this capture is for the currently-selected agent, the
                // pane content doubles as preview material.
                let is_selected = self
                    .selected_agent()
                    .is_some_and(|a| a.session_name == session_name);
                if is_selected
                    && let Some(c) = content
                    && self.preview_content.as_deref() != Some(c.as_str())
                {
                    self.preview_content = Some(c);
                    self.dirty = true;
                }

                if let Some(agent) = self
                    .agents
                    .iter_mut()
                    .find(|a| a.session_name == session_name)
                {
                    // Attach/detach reflows the pane and changes capture-pane
                    // output even when the agent produced no new bytes. When
                    // the attached-client count changed since the last poll,
                    // reseed the hash without claiming activity.
                    let attach_changed = agent
                        .last_attached_count
                        .is_some_and(|prev| prev != attached_count);
                    agent.last_attached_count = Some(attached_count);

                    match agent.last_pane_hash {
                        None => {
                            // Seed last_pane_hash without claiming activity. The
                            // seen_activity_since_seed reset below means
                            // `seen_activity` answers "since the current seed",
                            // not "ever in this agent's lifetime" — so a
                            // post-detach reseed correctly forgets prior bursts
                            // and waits for new ones before showing a spinner
                            // for an agent that may have gone idle in the gap.
                            agent.last_pane_hash = Some(content_hash);
                            agent.quiet_captures = 0;
                            agent.seen_activity_since_seed = false;
                            agent.consecutive_emits = 0;
                        }
                        Some(_) if attach_changed => {
                            agent.last_pane_hash = Some(content_hash);
                            agent.quiet_captures = 0;
                            agent.seen_activity_since_seed = false;
                            agent.consecutive_emits = 0;
                        }
                        Some(prev) if prev == content_hash => {
                            agent.quiet_captures = agent.quiet_captures.saturating_add(1);
                            agent.consecutive_emits = 0;
                        }
                        Some(_) => {
                            agent.last_pane_hash = Some(content_hash);
                            agent.quiet_captures = 0;
                            agent.consecutive_emits = agent.consecutive_emits.saturating_add(1);
                            // Only claim real activity once consecutive emits
                            // confirm a sustained burst — single-capture blips
                            // (cursor blinks, terminal title updates, stray
                            // escape sequences after a finished agent) get
                            // filtered out here.
                            if agent.consecutive_emits >= crate::agent::EMIT_THRESHOLD {
                                agent.seen_activity_since_seed = true;
                            }
                            self.dirty = true;
                        }
                    }
                }
            }
            Action::BranchesLoaded {
                branches: new_branches,
            } => {
                let repos = self.config.resolved_repos();
                let worktree_branches: Vec<(PathBuf, String)> = self
                    .agents
                    .iter()
                    .map(|a| (a.repo_path.clone(), a.branch.clone()))
                    .collect();
                if let Mode::NewAgent {
                    branches,
                    base_index,
                    branch_name,
                    name_pristine,
                    existing_branches,
                    repo_index,
                    ..
                } = &mut self.mode
                {
                    let today = chrono_free_date_str();
                    *branch_name = generate_branch_name(&new_branches, &today);
                    *name_pristine = true;
                    *base_index = find_main_branch(&new_branches);

                    let repo_path = repos.get(*repo_index).cloned();
                    *existing_branches = new_branches
                        .iter()
                        .filter(|b| {
                            !worktree_branches.iter().any(|(rp, ab)| {
                                repo_path.as_ref().is_some_and(|r| r == rp) && ab == *b
                            })
                        })
                        .cloned()
                        .collect();

                    *branches = new_branches;
                }
            }

            Action::TogglePreview => {
                self.preview_mode = match self.preview_mode {
                    PreviewMode::Terminal => PreviewMode::MergeRequest,
                    PreviewMode::MergeRequest => PreviewMode::Terminal,
                };
            }
            Action::MrRefreshed { key, snapshot } => {
                self.mr_snapshots.insert(key, snapshot);
                self.mr_refresh_outstanding = self.mr_refresh_outstanding.saturating_sub(1);
                if self.mr_refresh_outstanding == 0 {
                    self.mr_refresh_pending = false;
                }
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
                if let (Some(key), Some(id_or_branch)) =
                    (self.selected_mr_key(), self.selected_mr_id_or_branch())
                {
                    cmds.push(Command::OpenMr { key, id_or_branch });
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
                            self.status_message =
                                Some("agent running; attach or stop first".into());
                        }
                        AgentStatus::Creating => {
                            self.status_message = Some("agent still creating".into());
                        }
                        AgentStatus::Error(_) => {
                            self.status_message =
                                Some("agent errored; delete or restart first".into());
                        }
                        AgentStatus::Stopped => {
                            let prompt = match intent {
                                MrIntent::Rebase => {
                                    crate::gitlab::rebase_prompt(&self.selected_base_branch())
                                }
                                MrIntent::MakeReady => {
                                    let Some(url) =
                                        self.selected_mr().and_then(|mr| mr.url.as_deref())
                                    else {
                                        self.status_message = Some("no MR".into());
                                        return cmds;
                                    };
                                    crate::gitlab::make_ready_prompt(url)
                                }
                                MrIntent::ReviewFix => {
                                    let Some(url) =
                                        self.selected_mr().and_then(|mr| mr.url.as_deref())
                                    else {
                                        self.status_message = Some("no MR".into());
                                        return cmds;
                                    };
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

            // --- System ---
            Action::Tick => {
                self.spinner_frame = self.spinner_frame.wrapping_add(1);

                // Walk agents once: fire the spinner→done notification edge,
                // update was_spinner_visible, and decide whether to repaint.
                // Repaint when any spinner is visible (animate it) OR any
                // agent's working state flipped (catch the working→done
                // transition frame, which would otherwise freeze the spinner
                // on its last glyph).
                let notify = self.should_notify();
                let mut any_visible = false;
                let mut any_change = false;
                for agent in self.agents.iter_mut() {
                    let visible_now = agent.shows_spinner();
                    let just_finished = agent.was_spinner_visible
                        && !visible_now
                        && matches!(agent.status, AgentStatus::Running)
                        && agent.seen_activity_since_seed;
                    if just_finished {
                        if notify {
                            notifications::fire(
                                &format!("{}/{}", agent.repo_name, agent.slug),
                                "agent finished working",
                            );
                        }
                        // Reset the activity latch so a single-capture blip
                        // arriving after this edge can't re-fire the
                        // notification: the next genuine burst will need
                        // EMIT_THRESHOLD consecutive emits to flip
                        // seen_activity_since_seed back on.
                        agent.seen_activity_since_seed = false;
                    }
                    if visible_now {
                        any_visible = true;
                    }
                    if visible_now != agent.was_spinner_visible {
                        any_change = true;
                    }
                    agent.was_spinner_visible = visible_now;
                }
                if any_visible || any_change {
                    self.dirty = true;
                }

                // Activity capture: every 5th tick (~500ms), one per session-having
                // agent. Drives sub-second "done" detection via content-hash deltas
                // (replaces the coarse-grained tmux window_activity timestamp), and
                // the selected agent's capture doubles as preview content.
                if self.spinner_frame.is_multiple_of(5) {
                    for agent in &self.agents {
                        if agent.status.has_session() {
                            cmds.push(Command::CaptureActivity {
                                session_name: agent.session_name.clone(),
                            });
                        }
                    }
                }

                // Rediscover agents every 30th tick (~3s), with backpressure.
                // Runs in every mode: modal flows (e.g. the new-agent wizard) take
                // seconds to navigate, and without rediscovery the activity
                // timestamps go stale, flipping live agents to a checkmark and
                // firing spurious "agent finished working" notifications.
                if self.spinner_frame.is_multiple_of(30) && !self.discover_pending {
                    self.discover_pending = true;
                    cmds.push(Command::Discover(self.config.resolved_repos()));
                }

                if self.spinner_frame.is_multiple_of(100) {
                    cmds.extend(self.schedule_mr_refresh());
                }
            }
            Action::Quit => {
                self.should_quit = true;
            }
            Action::TerminalFocus(focused) => {
                self.focused = focused;
            }

            Action::FocusNext => {
                if let Mode::NewAgent {
                    focus,
                    branch_mode,
                    branch_name,
                    branches,
                    name_pristine,
                    ..
                } = &mut self.mode
                {
                    if *focus == NewAgentFocus::Name && branch_name.is_empty() {
                        let today = chrono_free_date_str();
                        *branch_name = generate_branch_name(branches, &today);
                        *name_pristine = true;
                    }
                    *focus = match (&*focus, &*branch_mode) {
                        (NewAgentFocus::Agent, _) => NewAgentFocus::Repo,
                        (NewAgentFocus::Repo, _) => NewAgentFocus::BranchToggle,
                        (NewAgentFocus::BranchToggle, _) => NewAgentFocus::BranchList,
                        (NewAgentFocus::BranchList, BranchMode::New) => NewAgentFocus::Name,
                        (NewAgentFocus::BranchList, BranchMode::Existing) => NewAgentFocus::Prompt,
                        (NewAgentFocus::Name, _) => NewAgentFocus::Prompt,
                        (NewAgentFocus::Prompt, _) => NewAgentFocus::Agent,
                    };
                }
            }
            Action::FocusPrev => {
                if let Mode::NewAgent {
                    focus,
                    branch_mode,
                    branch_name,
                    branches,
                    name_pristine,
                    ..
                } = &mut self.mode
                {
                    if *focus == NewAgentFocus::Name && branch_name.is_empty() {
                        let today = chrono_free_date_str();
                        *branch_name = generate_branch_name(branches, &today);
                        *name_pristine = true;
                    }
                    *focus = match (&*focus, &*branch_mode) {
                        (NewAgentFocus::Agent, _) => NewAgentFocus::Prompt,
                        (NewAgentFocus::Repo, _) => NewAgentFocus::Agent,
                        (NewAgentFocus::BranchToggle, _) => NewAgentFocus::Repo,
                        (NewAgentFocus::BranchList, _) => NewAgentFocus::BranchToggle,
                        (NewAgentFocus::Name, _) => NewAgentFocus::BranchList,
                        (NewAgentFocus::Prompt, BranchMode::New) => NewAgentFocus::Name,
                        (NewAgentFocus::Prompt, BranchMode::Existing) => NewAgentFocus::BranchList,
                    };
                }
            }
        }
        cmds
    }

    /// Map crossterm key events to Actions
    pub fn handle_key(&self, key: crossterm::event::KeyEvent) -> Option<Action> {
        use crossterm::event::KeyCode;

        if key.kind != crossterm::event::KeyEventKind::Press {
            return None;
        }

        match &self.mode {
            Mode::Normal => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
                KeyCode::Char('j') | KeyCode::Down => Some(Action::MoveDown),
                KeyCode::Char('k') | KeyCode::Up => Some(Action::MoveUp),
                KeyCode::Char('n') => Some(Action::StartNewAgent),
                KeyCode::Char('a') | KeyCode::Enter => Some(Action::Attach),
                KeyCode::Tab => Some(Action::TogglePreview),
                KeyCode::Char('m') => Some(Action::MrCreate),
                KeyCode::Char('M') => Some(Action::MrMerge),
                KeyCode::Char('o') => Some(Action::MrOpen),
                KeyCode::Char('r') => Some(Action::MrIntent(MrIntent::Rebase)),
                KeyCode::Char('f') => Some(Action::MrIntent(MrIntent::MakeReady)),
                KeyCode::Char('v') => Some(Action::MrIntent(MrIntent::ReviewFix)),
                KeyCode::Char('x') => self
                    .selected_agent()
                    .filter(|a| a.status.has_session())
                    .map(|a| Action::KillSession(a.session_name.clone())),
                KeyCode::Char('d') => Some(Action::StartDelete),
                KeyCode::Char('?') => Some(Action::ToggleHelp),
                _ => None,
            },
            Mode::ConfirmDelete => match key.code {
                KeyCode::Esc => Some(Action::CancelMode),
                KeyCode::Char('q') => Some(Action::CancelMode),
                KeyCode::Char('y') => Some(Action::DeleteAll {
                    preserve_tmux: false,
                }),
                KeyCode::Char('p') => Some(Action::DeleteAll {
                    preserve_tmux: true,
                }),
                _ => None,
            },
            Mode::ConfirmMerge => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => Some(Action::CancelMode),
                KeyCode::Char('y') => Some(Action::MrMergeConfirmed),
                _ => None,
            },
            Mode::NewAgent { focus, .. } => match key.code {
                KeyCode::Esc => Some(Action::CancelMode),
                KeyCode::Enter
                    if key.modifiers.contains(crossterm::event::KeyModifiers::ALT)
                        && matches!(focus, NewAgentFocus::Prompt) =>
                {
                    Some(Action::TypeChar('\n'))
                }
                KeyCode::Enter => Some(Action::PickerConfirm),
                KeyCode::Tab => Some(Action::FocusNext),
                KeyCode::BackTab => Some(Action::FocusPrev),
                // Horizontal fields: Repo, BranchToggle
                KeyCode::Left
                    if matches!(
                        focus,
                        NewAgentFocus::Agent | NewAgentFocus::Repo | NewAgentFocus::BranchToggle
                    ) =>
                {
                    Some(Action::PickerPrev)
                }
                KeyCode::Right
                    if matches!(
                        focus,
                        NewAgentFocus::Agent | NewAgentFocus::Repo | NewAgentFocus::BranchToggle
                    ) =>
                {
                    Some(Action::PickerNext)
                }
                // Vertical field: BranchList
                KeyCode::Up if matches!(focus, NewAgentFocus::BranchList) => {
                    Some(Action::PickerPrev)
                }
                KeyCode::Down if matches!(focus, NewAgentFocus::BranchList) => {
                    Some(Action::PickerNext)
                }
                KeyCode::Char('k') if matches!(focus, NewAgentFocus::BranchList) => {
                    Some(Action::PickerPrev)
                }
                KeyCode::Char('j') if matches!(focus, NewAgentFocus::BranchList) => {
                    Some(Action::PickerNext)
                }
                // Text fields: Name, Prompt
                KeyCode::Backspace
                    if matches!(focus, NewAgentFocus::Prompt | NewAgentFocus::Name) =>
                {
                    Some(Action::TypeBackspace)
                }
                KeyCode::Char('q')
                    if !matches!(focus, NewAgentFocus::Prompt | NewAgentFocus::Name) =>
                {
                    Some(Action::CancelMode)
                }
                KeyCode::Char(c)
                    if matches!(focus, NewAgentFocus::Prompt | NewAgentFocus::Name) =>
                {
                    Some(Action::TypeChar(c))
                }
                _ => None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyCode;

    fn test_app() -> App {
        let toml_str = r#"repos = ["~/src/myapp"]"#;
        let config = crate::config::Config::from_toml_str(toml_str).unwrap();
        App::new(config)
    }

    fn test_app_with_repos(repos: &[&str]) -> App {
        let repos_toml: Vec<String> = repos.iter().map(|r| format!("\"{r}\"")).collect();
        let toml_str = format!("repos = [{}]", repos_toml.join(", "));
        let config = crate::config::Config::from_toml_str(&toml_str).unwrap();
        App::new(config)
    }

    #[test]
    fn move_down_increments_selected() {
        let mut app = test_app();
        app.agents = vec![mock_agent("a"), mock_agent("b")];
        app.update(Action::MoveDown);
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn move_down_clamps_at_end() {
        let mut app = test_app();
        app.agents = vec![mock_agent("a")];
        app.update(Action::MoveDown);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn move_up_decrements_selected() {
        let mut app = test_app();
        app.agents = vec![mock_agent("a"), mock_agent("b")];
        app.selected = 1;
        app.update(Action::MoveUp);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn agents_refreshed_replaces_list() {
        let mut app = test_app();
        let agents = vec![mock_agent("fix-auth")];
        app.update(Action::AgentsRefreshed(agents));
        assert_eq!(app.agents.len(), 1);
        assert_eq!(app.agents[0].branch, "fix-auth");
    }

    #[test]
    fn agent_ready_updates_status_and_worktree_path() {
        let mut app = test_app();
        let mut creating = mock_agent_creating("fix-auth");
        creating.worktree_path = PathBuf::new(); // PickerConfirm leaves it empty
        app.agents = vec![creating];
        let path = PathBuf::from("/tmp/myapp-worktrees/fix-auth");
        app.update(Action::AgentReady {
            branch: "fix-auth".into(),
            session: "z-myapp-fix-auth".into(),
            worktree_path: path.clone(),
        });
        assert!(matches!(
            app.agents[0].status,
            crate::agent::AgentStatus::Running
        ));
        assert_eq!(app.agents[0].worktree_path, path);
    }

    #[test]
    fn agent_failed_updates_status() {
        let mut app = test_app();
        app.agents = vec![mock_agent_creating("fix-auth")];
        app.update(Action::AgentFailed {
            session: "z-myapp-fix-auth".into(),
            error: "already exists".into(),
        });
        assert!(matches!(
            app.agents[0].status,
            crate::agent::AgentStatus::Error(_)
        ));
    }

    #[test]
    fn tick_records_working_agents_into_was_spinner_visible() {
        let mut app = test_app();
        // Active agent: hash seeded, real activity observed, well under
        // the quiet threshold → shows_spinner() = true.
        let mut agent = mock_agent("fix-auth");
        agent.status = AgentStatus::Running;
        agent.last_pane_hash = Some(0x1);
        agent.seen_activity_since_seed = true;
        agent.quiet_captures = 0;
        app.agents = vec![agent];
        app.update(Action::Tick);
        assert!(app.agents[0].was_spinner_visible);
    }

    #[test]
    fn tick_does_not_show_spinner_for_freshly_discovered_idle_agent() {
        // Repro for "every agent shows as working at startup" UX bug.
        // A newly-discovered agent has last_pane_hash = None and no
        // observed activity; under the corrected predicate, it stays
        // idle (was_spinner_visible defaults to false on construction).
        let mut app = test_app();
        let mut agent = mock_agent("fix-auth");
        agent.status = AgentStatus::Running;
        agent.last_pane_hash = None;
        agent.seen_activity_since_seed = false;
        agent.was_spinner_visible = false;
        app.agents = vec![agent];
        app.update(Action::Tick);
        assert!(
            !app.agents[0].was_spinner_visible,
            "freshly-discovered agent must not flash a spinner"
        );
    }

    #[test]
    fn tick_does_not_show_spinner_for_idle_agent_after_detach() {
        // Repro for "leaving a static/idle agent session temporarily
        // shows it as working again." Pre-detach, the agent was idle
        // (was_spinner_visible = false, even though seen_activity was
        // historically true). Detach clears last_pane_hash. The next
        // Tick must keep the spinner off; the next capture must not
        // resurrect it via the seed.
        let mut app = test_app();
        let mut agent = mock_agent("fix-auth");
        agent.status = AgentStatus::Running;
        agent.last_pane_hash = None; // post-detach state
        agent.quiet_captures = 0;
        agent.seen_activity_since_seed = true; // legacy latch from earlier
        agent.was_spinner_visible = false; // we were already idle pre-detach
        app.agents = vec![agent];
        app.update(Action::Tick);
        assert!(
            !app.agents[0].was_spinner_visible,
            "post-detach idle agent must stay idle through the unobserved window"
        );

        // Simulate the next capture seeding the hash. seen_activity must
        // reset so the new seed doesn't pretend prior activity continues.
        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0xc0ffee,
            attached_count: 0,
        });
        app.update(Action::Tick);
        assert!(
            !app.agents[0].was_spinner_visible,
            "first post-detach capture must not resurrect the spinner"
        );
    }

    #[test]
    fn tick_clears_was_spinner_visible_when_agent_goes_idle() {
        let mut app = test_app();
        let mut agent = mock_agent("fix-auth");
        // Observation-idle: pane hash seeded and quiet_captures hit threshold
        // → shows_spinner = false.
        agent.status = AgentStatus::Running;
        agent.last_pane_hash = Some(0x1);
        agent.quiet_captures = crate::agent::QUIET_THRESHOLD;
        agent.seen_activity_since_seed = true;
        // Seed: pretend the agent was working last tick.
        agent.was_spinner_visible = true;
        app.agents = vec![agent];
        app.update(Action::Tick);
        // Edge detected → flag flipped off, latch reset.
        assert!(!app.agents[0].was_spinner_visible);
        assert!(
            !app.agents[0].seen_activity_since_seed,
            "spinner→done edge resets the activity latch so post-edge \
             blips can't re-fire the notification"
        );
    }

    #[test]
    fn post_done_single_blip_does_not_refire_notification() {
        // Regression for: agent finished, notification fired, then a
        // single-capture transient (cursor blink, terminal title rewrite,
        // stray escape) caused the spinner to return briefly and then
        // fire a SECOND "done" notification ~3.5s later.
        //
        // After the spinner→done edge, seen_activity is reset and a
        // single emit only sets consecutive_emits = 1 (under the
        // EMIT_THRESHOLD = 2 confirmation). seen_activity stays false,
        // shows_spinner stays false (was_spinner_visible is false post-edge),
        // and no second edge can fire.
        let mut app = test_app();
        let mut agent = mock_agent("fix-auth");
        agent.status = AgentStatus::Running;
        agent.last_pane_hash = Some(0xf1_u64);
        agent.quiet_captures = crate::agent::QUIET_THRESHOLD;
        agent.seen_activity_since_seed = true;
        agent.was_spinner_visible = true;
        app.agents = vec![agent];

        // First Tick: edge fires (would fire notification if enabled),
        // resets seen_activity, flips was_spinner_visible off.
        app.update(Action::Tick);
        assert!(!app.agents[0].was_spinner_visible);
        assert!(!app.agents[0].seen_activity_since_seed);

        // Single-capture blip: hash changes once. consecutive_emits
        // becomes 1 — below EMIT_THRESHOLD — so seen_activity stays
        // false, and the spinner does NOT come back on.
        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0xb1_u64,
            attached_count: 0,
        });
        assert_eq!(app.agents[0].consecutive_emits, 1);
        assert!(
            !app.agents[0].seen_activity_since_seed,
            "single-capture blip must not re-claim activity"
        );

        // Subsequent quiet captures: the blip's new hash stays put.
        // shows_spinner stays false; no second edge to fire.
        for _ in 0..crate::agent::QUIET_THRESHOLD {
            app.update(Action::ActivityCaptured {
                session_name: "z-myapp-fix-auth".into(),
                content: None,
                content_hash: 0xb1_u64,
                attached_count: 0,
            });
            app.update(Action::Tick);
            assert!(
                !app.agents[0].was_spinner_visible,
                "post-done single-blip must never resurrect the spinner"
            );
        }
    }

    #[test]
    fn post_done_sustained_burst_re_arms_notification() {
        // Counterpoint: if the agent actually starts NEW work after
        // finishing (sustained hash changes, not a blip), the next
        // quiet should fire a fresh notification. Verifies the latch
        // is re-armed by EMIT_THRESHOLD consecutive emits.
        let mut app = test_app();
        let mut agent = mock_agent("fix-auth");
        agent.status = AgentStatus::Running;
        agent.last_pane_hash = Some(0xf1_u64);
        agent.quiet_captures = crate::agent::QUIET_THRESHOLD;
        agent.seen_activity_since_seed = true;
        agent.was_spinner_visible = true;
        app.agents = vec![agent];

        app.update(Action::Tick); // edge fires; seen_activity reset.

        // Two consecutive hash deltas confirm a real new burst.
        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0xa1_u64,
            attached_count: 0,
        });
        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0xa2_u64,
            attached_count: 0,
        });
        assert!(
            app.agents[0].seen_activity_since_seed,
            "EMIT_THRESHOLD consecutive emits re-arm the activity latch"
        );

        // Tick: spinner is back on.
        app.update(Action::Tick);
        assert!(app.agents[0].was_spinner_visible);
    }

    #[test]
    fn tick_uses_shows_spinner_for_working_set() {
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.status = AgentStatus::Running;
        a.last_pane_hash = Some(0x1);
        a.seen_activity_since_seed = true;
        a.quiet_captures = 0; // observation says: just emitted
        app.agents = vec![a];

        app.update(Action::Tick);

        // Working is determined by quiet_captures < QUIET_THRESHOLD, not
        // by any timestamp on AgentStatus::Running.
        assert!(app.agents[0].was_spinner_visible);
    }

    #[test]
    fn event_loop_gap_does_not_fire_spurious_done_notification() {
        // Simulates the bug class: an agent was working, the event loop
        // stalled (tmux attach, OS suspend, anything that prevents Ticks and
        // captures), and we resume. The observation counter doesn't tick
        // forward without captures, so the gap is invisible — no false edge.
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.status = AgentStatus::Running;
        a.last_pane_hash = Some(0x1);
        a.quiet_captures = 2; // mid-flight, well under threshold
        a.seen_activity_since_seed = true;
        a.was_spinner_visible = true;
        app.agents = vec![a];

        // ...arbitrarily long gap with no captures, no Ticks fired during it...

        // First Tick after the gap:
        app.update(Action::Tick);

        // Agent must still be considered working (counter unchanged at 2).
        assert!(
            app.agents[0].was_spinner_visible,
            "no captures during gap → no false 'done' edge"
        );
    }

    #[test]
    fn detach_then_resume_does_not_fire_spurious_done_notification() {
        // Specific repro of the original detach bug, expressed in the new
        // model. After detach, last_pane_hash is cleared (existing reseed
        // contract). Tick must keep was_spinner_visible true because the
        // unobserved-state branch of shows_spinner() returns true.
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.status = AgentStatus::Running;
        a.last_pane_hash = Some(0x1234_5678_u64);
        a.quiet_captures = 0;
        a.seen_activity_since_seed = true;
        a.was_spinner_visible = true;
        app.agents = vec![a];

        // Simulate the detach reseed (Task 5.2 simplifies this to just two
        // field writes; for now we still call on_session_detached).
        app.on_session_detached("z-myapp-fix-auth");

        app.update(Action::Tick);

        assert!(
            app.agents[0].was_spinner_visible,
            "post-detach Tick must keep agent in working state"
        );
    }

    #[test]
    fn notification_edge_fires_when_was_spinner_visible_flips_off_with_observed_activity() {
        // Setup: agent had a real activity burst, then went quiet past
        // threshold. The Tick must fire a notification on this transition.
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.status = AgentStatus::Running;
        a.last_pane_hash = Some(0x1);
        a.quiet_captures = crate::agent::QUIET_THRESHOLD;
        a.seen_activity_since_seed = true;
        a.was_spinner_visible = true; // last tick we were spinning
        app.agents = vec![a];
        // Notifications enabled, terminal unfocused so they fire:
        app.config.notifications.enabled = true;
        app.config.notifications.only_when_unfocused = true;
        app.focused = false;

        app.update(Action::Tick);

        // After Tick: was_spinner_visible should now be false (no more spinner).
        assert!(!app.agents[0].was_spinner_visible);
    }

    #[test]
    fn notification_edge_does_not_fire_for_freshly_discovered_idle_agent() {
        // Repro for the "discovered already idle" false positive: agent
        // appears, has no prior activity, transitions from "spinner because
        // unobserved" to "no spinner because quiet_captures hit threshold".
        // No notification should fire.
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.status = AgentStatus::Running;
        a.last_pane_hash = Some(0x1);
        a.quiet_captures = crate::agent::QUIET_THRESHOLD;
        a.seen_activity_since_seed = false; // never observed real activity
        a.was_spinner_visible = true; // was showing spinner last tick
        app.agents = vec![a];

        // (Can't easily assert "no notification fired" without mocking
        // notifications::fire. Instead, observable post-Tick state:
        // was_spinner_visible should now be false. The actual gating is
        // in the implementation; this test ensures was_spinner_visible
        // is correctly updated even when notifications would be suppressed.)
        app.update(Action::Tick);
        assert!(!app.agents[0].was_spinner_visible);
    }

    #[test]
    fn notification_edge_does_not_fire_when_status_is_not_active() {
        // Edge guard: even with was_spinner_visible flipping off, agents in
        // Stopped/Error/Creating states must not fire the "agent finished"
        // edge.
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.status = AgentStatus::Stopped;
        a.last_pane_hash = Some(0x1);
        a.quiet_captures = crate::agent::QUIET_THRESHOLD;
        a.seen_activity_since_seed = true;
        a.was_spinner_visible = true;
        app.agents = vec![a];

        app.update(Action::Tick);
        assert!(!app.agents[0].was_spinner_visible);
    }

    #[test]
    fn tick_drops_agent_from_working_set_at_quiet_threshold() {
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.status = AgentStatus::Running;
        a.last_pane_hash = Some(0x1);
        a.quiet_captures = crate::agent::QUIET_THRESHOLD;
        app.agents = vec![a];

        app.update(Action::Tick);

        assert!(
            !app.agents[0].was_spinner_visible,
            "quiet_captures at threshold drops the agent from the working set"
        );
    }

    #[test]
    fn terminal_focus_action_updates_focused_flag() {
        let mut app = test_app();
        assert!(app.focused);
        app.update(Action::TerminalFocus(false));
        assert!(!app.focused);
        app.update(Action::TerminalFocus(true));
        assert!(app.focused);
    }

    #[test]
    fn should_notify_respects_config_and_focus() {
        let mut app = test_app();
        // Default config: notifications disabled → never fires.
        app.focused = false;
        assert!(!app.should_notify());

        app.config.notifications.enabled = true;
        // only_when_unfocused defaults true; focused → suppress.
        app.focused = true;
        assert!(!app.should_notify());
        app.focused = false;
        assert!(app.should_notify());

        // Disable focus gating: focus state is irrelevant.
        app.config.notifications.only_when_unfocused = false;
        app.focused = true;
        assert!(app.should_notify());
    }

    #[test]
    fn delete_failed_surfaces_error_in_status_bar() {
        let mut app = test_app();
        app.update(Action::DeleteFailed {
            branch: "fix-auth".into(),
            error: "worktree: not a working tree".into(),
        });
        let msg = app.status_message.as_deref().unwrap_or("");
        assert!(
            msg.contains("fix-auth"),
            "expected branch in status: {msg:?}"
        );
        assert!(
            msg.contains("not a working tree"),
            "expected error in status: {msg:?}"
        );
    }

    #[test]
    fn quit_sets_flag() {
        let mut app = test_app();
        app.update(Action::Quit);
        assert!(app.should_quit);
    }

    #[test]
    fn find_main_branch_prefers_main() {
        let branches = vec!["develop".into(), "main".into(), "master".into()];
        assert_eq!(find_main_branch(&branches), 1);
    }

    #[test]
    fn find_main_branch_falls_back_to_master() {
        let branches = vec!["develop".into(), "master".into()];
        assert_eq!(find_main_branch(&branches), 1);
    }

    #[test]
    fn find_main_branch_defaults_to_zero() {
        let branches = vec!["develop".into(), "staging".into()];
        assert_eq!(find_main_branch(&branches), 0);
    }

    fn mock_agent(branch: &str) -> crate::agent::Agent {
        let slug = branch.replace('/', "-");
        crate::agent::Agent {
            repo_path: "/tmp/repo".into(),
            repo_name: "myapp".into(),
            branch: branch.into(),
            base_branch: None,
            worktree_path: format!("/tmp/repo-worktrees/{branch}").into(),
            slug: slug.clone(),
            session_name: format!("z-myapp-{slug}"),
            status: crate::agent::AgentStatus::Running,
            agent_name: "codex".into(),
            last_pane_hash: None,
            last_attached_count: Some(0),
            quiet_captures: 0,
            seen_activity_since_seed: false,
            was_spinner_visible: false,
            consecutive_emits: 0,
        }
    }

    fn mock_agent_creating(branch: &str) -> crate::agent::Agent {
        let mut a = mock_agent(branch);
        a.status = crate::agent::AgentStatus::Creating;
        a
    }

    #[test]
    fn generate_branch_name_first_of_day() {
        let branches: Vec<String> = vec!["main".into(), "develop".into()];
        let name = generate_branch_name(&branches, "0409");
        assert_eq!(name, "z-0409-1");
    }

    #[test]
    fn generate_branch_name_increments() {
        let branches: Vec<String> = vec!["main".into(), "z-0409-1".into(), "z-0409-2".into()];
        let name = generate_branch_name(&branches, "0409");
        assert_eq!(name, "z-0409-3");
    }

    #[test]
    fn generate_branch_name_ignores_other_dates() {
        let branches: Vec<String> = vec!["z-0408-5".into(), "z-0409-1".into()];
        let name = generate_branch_name(&branches, "0409");
        assert_eq!(name, "z-0409-2");
    }

    #[test]
    fn start_new_agent_enters_new_agent_mode() {
        let mut app = test_app();
        app.update(Action::StartNewAgent);
        assert!(matches!(app.mode, Mode::NewAgent { .. }));
    }

    fn test_app_in_new_agent_mode() -> App {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Prompt,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        app
    }

    fn make_key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
        crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    #[test]
    fn question_mark_toggles_help_in_normal_mode() {
        let mut app = test_app();
        assert!(!app.help_visible);
        let action = app.handle_key(make_key(KeyCode::Char('?'))).unwrap();
        assert!(matches!(action, Action::ToggleHelp));
        app.update(action);
        assert!(app.help_visible);
        let action = app.handle_key(make_key(KeyCode::Char('?'))).unwrap();
        app.update(action);
        assert!(!app.help_visible);
    }

    #[test]
    fn normal_tab_toggles_preview() {
        let app = test_app();
        let action = app.handle_key(make_key(KeyCode::Tab));
        assert!(matches!(action, Some(Action::TogglePreview)));
    }

    #[test]
    fn normal_m_creates_mr() {
        let app = test_app();
        let action = app.handle_key(make_key(KeyCode::Char('m')));
        assert!(matches!(action, Some(Action::MrCreate)));
    }

    #[test]
    fn normal_shift_m_starts_mr_merge() {
        let app = test_app();
        let action = app.handle_key(make_key(KeyCode::Char('M')));
        assert!(matches!(action, Some(Action::MrMerge)));
    }

    #[test]
    fn normal_o_opens_mr() {
        let app = test_app();
        let action = app.handle_key(make_key(KeyCode::Char('o')));
        assert!(matches!(action, Some(Action::MrOpen)));
    }

    #[test]
    fn normal_r_starts_rebase_intent() {
        let app = test_app();
        let action = app.handle_key(make_key(KeyCode::Char('r')));
        assert!(matches!(action, Some(Action::MrIntent(MrIntent::Rebase))));
    }

    #[test]
    fn normal_f_starts_make_ready_intent() {
        let app = test_app();
        let action = app.handle_key(make_key(KeyCode::Char('f')));
        assert!(matches!(
            action,
            Some(Action::MrIntent(MrIntent::MakeReady))
        ));
    }

    #[test]
    fn normal_v_starts_review_fix_intent() {
        let app = test_app();
        let action = app.handle_key(make_key(KeyCode::Char('v')));
        assert!(matches!(
            action,
            Some(Action::MrIntent(MrIntent::ReviewFix))
        ));
    }

    #[test]
    fn confirmmerge_y_confirms_merge() {
        let mut app = test_app();
        app.mode = Mode::ConfirmMerge;
        let action = app.handle_key(make_key(KeyCode::Char('y')));
        assert!(matches!(action, Some(Action::MrMergeConfirmed)));
    }

    #[test]
    fn confirmmerge_esc_cancels() {
        let mut app = test_app();
        app.mode = Mode::ConfirmMerge;
        let action = app.handle_key(make_key(KeyCode::Esc));
        assert!(matches!(action, Some(Action::CancelMode)));
    }

    #[test]
    fn confirmmerge_q_cancels() {
        let mut app = test_app();
        app.mode = Mode::ConfirmMerge;
        let action = app.handle_key(make_key(KeyCode::Char('q')));
        assert!(matches!(action, Some(Action::CancelMode)));
    }

    #[test]
    fn confirmmerge_ignores_other_keys() {
        let mut app = test_app();
        app.mode = Mode::ConfirmMerge;
        let action = app.handle_key(make_key(KeyCode::Char('n')));
        assert!(action.is_none());
    }

    #[test]
    fn new_agent_typing_edits_prompt() {
        let app = test_app_in_new_agent_mode();
        let action = app.handle_key(make_key(KeyCode::Char('h')));
        assert!(matches!(action, Some(Action::TypeChar('h'))));
    }

    #[test]
    fn new_agent_enter_confirms() {
        let app = test_app_in_new_agent_mode();
        let action = app.handle_key(make_key(KeyCode::Enter));
        assert!(matches!(action, Some(Action::PickerConfirm)));
    }

    #[test]
    fn new_agent_tab_cycles_focus() {
        let app = test_app_in_new_agent_mode();
        let action = app.handle_key(make_key(KeyCode::Tab));
        assert!(matches!(action, Some(Action::FocusNext)));
    }

    #[test]
    fn new_agent_esc_cancels() {
        let app = test_app_in_new_agent_mode();
        let action = app.handle_key(make_key(KeyCode::Esc));
        assert!(matches!(action, Some(Action::CancelMode)));
    }

    #[test]
    fn new_agent_full_flow_emits_create_command() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Prompt,
            base_index: 0,
            branches: vec!["main".into(), "develop".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        // Type "fix"
        app.update(Action::TypeChar('f'));
        app.update(Action::TypeChar('i'));
        app.update(Action::TypeChar('x'));
        // Confirm
        let cmds = app.update(Action::PickerConfirm);
        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            Command::CreateAgent {
                branch,
                new_branch,
                base_branch,
                agent_name,
                fresh_cmd,
                ..
            } => {
                assert_eq!(branch, "z-0409-1");
                assert!(*new_branch);
                assert_eq!(*base_branch, Some("main".into()));
                assert_eq!(agent_name, "codex");
                assert_eq!(
                    fresh_cmd,
                    "codex --dangerously-bypass-approvals-and-sandbox 'fix'",
                );
            }
            other => panic!("expected CreateAgent, got {:?}", other),
        }
    }

    #[test]
    fn new_agent_focus_and_cycle_base() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: "test".into(),
            focus: NewAgentFocus::BranchList,
            base_index: 0,
            branches: vec!["main".into(), "develop".into(), "staging".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        // Already at BranchList (closest equivalent to old Base)
        if let Mode::NewAgent { focus, .. } = &app.mode {
            assert_eq!(*focus, NewAgentFocus::BranchList);
        }
        // Cycle base forward
        app.update(Action::PickerNext);
        if let Mode::NewAgent { base_index, .. } = &app.mode {
            assert_eq!(*base_index, 1);
        }
        app.update(Action::PickerNext);
        if let Mode::NewAgent { base_index, .. } = &app.mode {
            assert_eq!(*base_index, 2);
        }
        app.update(Action::PickerNext);
        if let Mode::NewAgent { base_index, .. } = &app.mode {
            assert_eq!(*base_index, 0);
        }
    }

    #[test]
    fn new_agent_empty_prompt_sends_none() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Prompt,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        let cmds = app.update(Action::PickerConfirm);
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            Command::CreateAgent { fresh_cmd, .. } => {
                // Empty prompt: fresh_cmd is the agent's bare cmd, no quoted prompt.
                assert_eq!(
                    *fresh_cmd,
                    "codex --dangerously-bypass-approvals-and-sandbox"
                );
            }
            other => panic!("expected CreateAgent, got {:?}", other),
        }
    }

    #[test]
    fn branches_loaded_updates_new_agent_mode() {
        let today = chrono_free_date_str();
        let existing_branch = format!("z-{today}-1");
        let expected_branch = format!("z-{today}-2");
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: "fix auth".into(),
            focus: NewAgentFocus::Prompt,
            base_index: 0,
            branches: Vec::new(),
            existing_branches: Vec::new(),
            branch_name: format!("z-{today}-1"),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        app.update(Action::BranchesLoaded {
            branches: vec!["develop".into(), "main".into(), existing_branch.clone()],
        });
        if let Mode::NewAgent {
            branches,
            base_index,
            branch_name,
            ..
        } = &app.mode
        {
            assert_eq!(
                branches,
                &vec!["develop".to_string(), "main".to_string(), existing_branch]
            );
            assert_eq!(*base_index, 1); // "main" is at index 1
            assert_eq!(*branch_name, expected_branch); // existing branch exists, so next is 2
        } else {
            panic!("expected NewAgent mode");
        }
    }

    #[test]
    fn focus_cycles_through_all_six_states() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Agent,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        let expected = vec![
            NewAgentFocus::Repo,
            NewAgentFocus::BranchToggle,
            NewAgentFocus::BranchList,
            NewAgentFocus::Name,
            NewAgentFocus::Prompt,
            NewAgentFocus::Agent,
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
    fn repo_cycling_changes_repo_index() {
        let mut app = test_app_with_repos(&["~/src/alpha", "~/src/beta", "~/src/gamma"]);
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Repo,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        app.update(Action::PickerNext);
        if let Mode::NewAgent { repo_index, .. } = &app.mode {
            assert_eq!(*repo_index, 1);
        }
        app.update(Action::PickerNext);
        if let Mode::NewAgent { repo_index, .. } = &app.mode {
            assert_eq!(*repo_index, 2);
        }
        app.update(Action::PickerNext);
        if let Mode::NewAgent { repo_index, .. } = &app.mode {
            assert_eq!(*repo_index, 0);
        }
    }

    #[test]
    fn new_agent_left_right_cycles_repo() {
        let mut app = test_app_with_repos(&["~/src/alpha", "~/src/beta"]);
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Repo,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        let action = app.handle_key(make_key(KeyCode::Right));
        assert!(matches!(action, Some(Action::PickerNext)));
        let action = app.handle_key(make_key(KeyCode::Left));
        assert!(matches!(action, Some(Action::PickerPrev)));
    }

    // Name-pristine (select-all-on-focus) tests

    #[test]
    fn name_pristine_first_char_replaces() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Name,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        app.update(Action::TypeChar('f'));
        if let Mode::NewAgent {
            branch_name,
            name_pristine,
            ..
        } = &app.mode
        {
            assert_eq!(branch_name, "f");
            assert!(!name_pristine);
        } else {
            panic!("expected NewAgent mode");
        }
    }

    #[test]
    fn name_pristine_backspace_clears() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Name,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        app.update(Action::TypeBackspace);
        if let Mode::NewAgent {
            branch_name,
            name_pristine,
            ..
        } = &app.mode
        {
            assert_eq!(branch_name, "");
            assert!(!name_pristine);
        } else {
            panic!("expected NewAgent mode");
        }
    }

    #[test]
    fn name_snap_back_on_empty_focus_away() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Name,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: String::new(),
            name_pristine: false,
            agent_name: "codex".to_string(),
        };
        // Tab away from empty Name field
        app.update(Action::FocusNext);
        if let Mode::NewAgent {
            branch_name,
            name_pristine,
            focus,
            ..
        } = &app.mode
        {
            assert!(
                !branch_name.is_empty(),
                "should have snapped back to generated name"
            );
            assert!(branch_name.starts_with("z-"));
            assert!(*name_pristine);
            assert_eq!(*focus, NewAgentFocus::Prompt);
        } else {
            panic!("expected NewAgent mode");
        }
    }

    #[test]
    fn name_not_pristine_appends() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Name,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "my-branch".into(),
            name_pristine: false,
            agent_name: "codex".to_string(),
        };
        app.update(Action::TypeChar('!'));
        if let Mode::NewAgent { branch_name, .. } = &app.mode {
            assert_eq!(branch_name, "my-branch!");
        } else {
            panic!("expected NewAgent mode");
        }
    }

    // Command-pattern tests
    #[test]
    fn refresh_returns_discover_command() {
        let mut app = test_app();
        let cmds = app.update(Action::RefreshAgents);
        assert_eq!(cmds.len(), 1);
        assert!(matches!(cmds[0], Command::Discover(_)));
    }

    #[test]
    fn tick_emits_discover_every_30_frames_with_backpressure() {
        let mut app = test_app();
        // First 29 ticks: no discover.
        for _ in 0..29 {
            let cmds = app.update(Action::Tick);
            assert!(!cmds.iter().any(|c| matches!(c, Command::Discover(_))));
        }
        // 30th tick: fires Discover and sets pending.
        let cmds = app.update(Action::Tick);
        assert!(cmds.iter().any(|c| matches!(c, Command::Discover(_))));
        assert!(app.discover_pending);

        // Subsequent 30th-frame ticks while pending: no new Discover.
        for _ in 0..30 {
            let cmds = app.update(Action::Tick);
            assert!(!cmds.iter().any(|c| matches!(c, Command::Discover(_))));
        }

        // After AgentsRefreshed clears the flag, next 30th tick fires again.
        app.update(Action::AgentsRefreshed(vec![]));
        assert!(!app.discover_pending);
        for _ in 0..29 {
            app.update(Action::Tick);
        }
        let cmds = app.update(Action::Tick);
        assert!(cmds.iter().any(|c| matches!(c, Command::Discover(_))));
    }

    #[test]
    fn tick_keeps_emitting_discover_in_new_agent_mode() {
        // Regression: opening the new-agent wizard used to halt rediscovery,
        // so live agents stopped getting their observation state refreshed.
        // They flipped to checkmarks and fired spurious "finished working"
        // notifications while the user was just tabbing through the wizard.
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Agent,
            base_index: 0,
            branches: Vec::new(),
            existing_branches: Vec::new(),
            branch_name: String::new(),
            name_pristine: true,
            agent_name: "claude".to_string(),
        };
        for _ in 0..29 {
            app.update(Action::Tick);
        }
        let cmds = app.update(Action::Tick);
        assert!(
            cmds.iter().any(|c| matches!(c, Command::Discover(_))),
            "expected Discover even while in NewAgent mode"
        );
    }

    #[test]
    fn move_down_returns_capture_for_new_selection() {
        let mut app = test_app();
        app.agents = vec![mock_agent("a"), mock_agent("b")];
        let cmds = app.update(Action::MoveDown);
        assert_eq!(app.selected, 1);
        assert!(matches!(cmds[0], Command::CaptureActivity { .. }));
    }

    #[test]
    fn focus_cycles_new_mode_six_states() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Repo,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        let expected = vec![
            NewAgentFocus::BranchToggle,
            NewAgentFocus::BranchList,
            NewAgentFocus::Name,
            NewAgentFocus::Prompt,
            NewAgentFocus::Agent,
            NewAgentFocus::Repo,
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
    fn focus_cycles_existing_mode_skips_name() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::Existing,
            prompt: String::new(),
            focus: NewAgentFocus::Repo,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: vec!["feature-auth".into()],
            branch_name: String::new(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        let expected = vec![
            NewAgentFocus::BranchToggle,
            NewAgentFocus::BranchList,
            NewAgentFocus::Prompt, // skips Name
            NewAgentFocus::Agent,
            NewAgentFocus::Repo,
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
    fn focus_prev_existing_mode_skips_name() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::Existing,
            prompt: String::new(),
            focus: NewAgentFocus::Prompt,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: vec!["feature-auth".into()],
            branch_name: String::new(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        app.update(Action::FocusPrev);
        if let Mode::NewAgent { focus, .. } = &app.mode {
            assert_eq!(*focus, NewAgentFocus::BranchList); // skips Name
        } else {
            panic!("expected NewAgent mode");
        }
    }

    #[test]
    fn picker_next_toggles_branch_mode() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::BranchToggle,
            base_index: 2,
            branches: vec!["main".into(), "develop".into(), "staging".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        app.update(Action::PickerNext);
        if let Mode::NewAgent {
            branch_mode,
            base_index,
            ..
        } = &app.mode
        {
            assert_eq!(*branch_mode, BranchMode::Existing);
            assert_eq!(*base_index, 0); // reset on toggle
        } else {
            panic!("expected NewAgent mode");
        }
    }

    #[test]
    fn picker_navigates_branch_list_vertically() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::BranchList,
            base_index: 0,
            branches: vec!["main".into(), "develop".into(), "staging".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        app.update(Action::PickerNext);
        if let Mode::NewAgent { base_index, .. } = &app.mode {
            assert_eq!(*base_index, 1);
        }
        app.update(Action::PickerNext);
        if let Mode::NewAgent { base_index, .. } = &app.mode {
            assert_eq!(*base_index, 2);
        }
        app.update(Action::PickerNext);
        if let Mode::NewAgent { base_index, .. } = &app.mode {
            assert_eq!(*base_index, 0); // wraps
        }
    }

    #[test]
    fn picker_navigates_existing_branch_list() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::Existing,
            prompt: String::new(),
            focus: NewAgentFocus::BranchList,
            base_index: 0,
            branches: vec!["main".into(), "develop".into(), "feature-auth".into()],
            existing_branches: vec!["feature-auth".into()],
            branch_name: String::new(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        app.update(Action::PickerNext);
        if let Mode::NewAgent { base_index, .. } = &app.mode {
            assert_eq!(*base_index, 0); // wraps at 1 (only 1 existing branch)
        }
    }

    #[test]
    fn picker_cycles_repo_on_repo_focus() {
        let mut app = test_app_with_repos(&["~/src/alpha", "~/src/beta"]);
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Repo,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        app.update(Action::PickerNext);
        if let Mode::NewAgent { repo_index, .. } = &app.mode {
            assert_eq!(*repo_index, 1);
        }
    }

    #[test]
    fn branch_list_up_down_maps_to_picker() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::BranchList,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        let action = app.handle_key(make_key(KeyCode::Down));
        assert!(matches!(action, Some(Action::PickerNext)));
        let action = app.handle_key(make_key(KeyCode::Up));
        assert!(matches!(action, Some(Action::PickerPrev)));
        // Left/right should NOT work for BranchList
        let action = app.handle_key(make_key(KeyCode::Left));
        assert!(action.is_none());
    }

    #[test]
    fn repo_left_right_maps_to_picker() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Repo,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        let action = app.handle_key(make_key(KeyCode::Right));
        assert!(matches!(action, Some(Action::PickerNext)));
        let action = app.handle_key(make_key(KeyCode::Left));
        assert!(matches!(action, Some(Action::PickerPrev)));
        // Up/down should NOT work for Repo
        let action = app.handle_key(make_key(KeyCode::Up));
        assert!(action.is_none());
    }

    #[test]
    fn branches_loaded_computes_existing_branches() {
        let mut app = test_app();
        let repos = app.config.resolved_repos();
        let mut agent = mock_agent("fix-auth");
        agent.repo_path = repos[0].clone();
        app.agents = vec![agent];
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::Existing,
            prompt: String::new(),
            focus: NewAgentFocus::BranchList,
            base_index: 0,
            branches: Vec::new(),
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        app.update(Action::BranchesLoaded {
            branches: vec![
                "main".into(),
                "develop".into(),
                "fix-auth".into(),
                "feature-new".into(),
            ],
        });
        if let Mode::NewAgent {
            existing_branches, ..
        } = &app.mode
        {
            // fix-auth has a worktree (in agents), so excluded
            assert!(existing_branches.contains(&"main".to_string()));
            assert!(existing_branches.contains(&"develop".to_string()));
            assert!(existing_branches.contains(&"feature-new".to_string()));
            assert!(!existing_branches.contains(&"fix-auth".to_string()));
        } else {
            panic!("expected NewAgent mode");
        }
    }

    #[test]
    fn branches_loaded_excludes_only_same_repo_worktrees() {
        let mut app = test_app_with_repos(&["~/src/alpha", "~/src/beta"]);
        let repos = app.config.resolved_repos();
        let mut agent = mock_agent("fix-auth");
        agent.repo_path = repos[1].clone(); // agent is on beta
        app.agents = vec![agent];
        app.mode = Mode::NewAgent {
            repo_index: 0, // wizard is on alpha
            branch_mode: BranchMode::Existing,
            prompt: String::new(),
            focus: NewAgentFocus::BranchList,
            base_index: 0,
            branches: Vec::new(),
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        app.update(Action::BranchesLoaded {
            branches: vec!["main".into(), "fix-auth".into()],
        });
        if let Mode::NewAgent {
            existing_branches, ..
        } = &app.mode
        {
            // fix-auth is on a different repo, so it should NOT be excluded
            assert!(existing_branches.contains(&"fix-auth".to_string()));
            assert_eq!(existing_branches.len(), 2);
        } else {
            panic!("expected NewAgent mode");
        }
    }

    #[test]
    fn picker_confirm_existing_mode_uses_selected_branch() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::Existing,
            prompt: "continue work".into(),
            focus: NewAgentFocus::Prompt,
            base_index: 1,
            branches: vec!["main".into(), "develop".into(), "feature-auth".into()],
            existing_branches: vec!["develop".into(), "feature-auth".into()],
            branch_name: String::new(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        let cmds = app.update(Action::PickerConfirm);
        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            Command::CreateAgent {
                branch,
                new_branch,
                base_branch,
                fresh_cmd,
                ..
            } => {
                assert_eq!(branch, "feature-auth"); // existing_branches[1]
                assert!(!new_branch);
                assert_eq!(*base_branch, None);
                assert_eq!(
                    *fresh_cmd,
                    "codex --dangerously-bypass-approvals-and-sandbox 'continue work'",
                );
            }
            other => panic!("expected CreateAgent, got {:?}", other),
        }
    }

    #[test]
    fn picker_confirm_existing_mode_empty_list_does_nothing() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::Existing,
            prompt: String::new(),
            focus: NewAgentFocus::Prompt,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: String::new(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        let cmds = app.update(Action::PickerConfirm);
        assert!(matches!(app.mode, Mode::NewAgent { .. })); // stays in mode
        assert!(cmds.is_empty());
    }

    #[test]
    fn quit_returns_no_commands() {
        let mut app = test_app();
        let cmds = app.update(Action::Quit);
        assert!(app.should_quit);
        assert!(cmds.is_empty());
    }

    #[test]
    fn attach_with_session_returns_attach_command() {
        let mut app = test_app();
        app.agents = vec![mock_agent("fix-auth")]; // mock_agent is Active
        let cmds = app.update(Action::Attach);
        assert_eq!(cmds.len(), 1);
        assert!(matches!(cmds[0], Command::Attach(_)));
    }

    #[test]
    fn attach_without_session_returns_prepare_attach() {
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.status = crate::agent::AgentStatus::Stopped;
        app.agents = vec![a];
        let cmds = app.update(Action::Attach);
        assert_eq!(cmds.len(), 1);
        assert!(matches!(cmds[0], Command::PrepareAttach { .. }));
        assert!(
            app.status_message
                .as_deref()
                .unwrap_or("")
                .contains("fix-auth")
        );
    }

    #[test]
    fn attach_ready_returns_attach_command() {
        let mut app = test_app();
        let agent = mock_agent("fix-auth");
        let cmds = app.update(Action::AttachReady(agent));
        assert_eq!(cmds.len(), 1);
        assert!(matches!(cmds[0], Command::Attach(_)));
    }

    #[test]
    fn delete_all_returns_delete_command_and_kills_tmux_by_default() {
        let mut app = test_app();
        app.agents = vec![mock_agent("fix-auth")];
        app.mode = Mode::ConfirmDelete;
        let cmds = app.update(Action::DeleteAll {
            preserve_tmux: false,
        });
        assert_eq!(cmds.len(), 1);
        assert!(matches!(
            cmds[0],
            Command::DeleteAgent {
                kill_session: true,
                ..
            }
        ));
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn delete_all_can_preserve_tmux_session() {
        let mut app = test_app();
        app.agents = vec![mock_agent("fix-auth")];
        app.mode = Mode::ConfirmDelete;
        let cmds = app.update(Action::DeleteAll {
            preserve_tmux: true,
        });
        assert_eq!(cmds.len(), 1);
        assert!(matches!(
            cmds[0],
            Command::DeleteAgent {
                kill_session: false,
                ..
            }
        ));
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn confirm_delete_y_cleans_tmux_by_default() {
        let mut app = test_app();
        app.mode = Mode::ConfirmDelete;
        let action = app.handle_key(make_key(KeyCode::Char('y')));
        assert!(matches!(
            action,
            Some(Action::DeleteAll {
                preserve_tmux: false
            })
        ));
    }

    #[test]
    fn confirm_delete_p_preserves_tmux() {
        let mut app = test_app();
        app.mode = Mode::ConfirmDelete;
        let action = app.handle_key(make_key(KeyCode::Char('p')));
        assert!(matches!(
            action,
            Some(Action::DeleteAll {
                preserve_tmux: true
            })
        ));
    }

    #[test]
    fn start_new_agent_begins_at_agent_focus() {
        let mut app = test_app();
        app.update(Action::StartNewAgent);
        if let Mode::NewAgent {
            focus, branch_mode, ..
        } = &app.mode
        {
            assert_eq!(*focus, NewAgentFocus::Agent);
            assert_eq!(*branch_mode, BranchMode::New);
        } else {
            panic!("expected NewAgent mode");
        }
    }

    #[test]
    fn picker_next_cycles_agent_name() {
        // Builtin agent order is ["claude", "codex"]. Starting at "claude",
        // next -> "codex", next -> "claude" (wraps).
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Agent,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "claude".to_string(),
        };
        app.update(Action::PickerNext);
        if let Mode::NewAgent { agent_name, .. } = &app.mode {
            assert_eq!(agent_name, "codex");
        } else {
            panic!("expected NewAgent mode");
        }
        app.update(Action::PickerNext);
        if let Mode::NewAgent { agent_name, .. } = &app.mode {
            assert_eq!(agent_name, "claude");
        } else {
            panic!("expected NewAgent mode");
        }
    }

    #[test]
    fn picker_prev_cycles_agent_name() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Agent,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "claude".to_string(),
        };
        // prev wraps backwards: claude -> codex
        app.update(Action::PickerPrev);
        if let Mode::NewAgent { agent_name, .. } = &app.mode {
            assert_eq!(agent_name, "codex");
        } else {
            panic!("expected NewAgent mode");
        }
    }

    #[test]
    fn picker_confirm_emits_selected_agent_name() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Prompt,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "claude".to_string(),
        };
        let cmds = app.update(Action::PickerConfirm);
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            Command::CreateAgent {
                agent_name,
                fresh_cmd,
                ..
            } => {
                assert_eq!(agent_name, "claude");
                assert_eq!(fresh_cmd, "claude --dangerously-skip-permissions");
            }
            other => panic!("expected CreateAgent, got {:?}", other),
        }
    }

    #[test]
    fn agent_left_right_maps_to_picker() {
        let mut app = test_app();
        app.mode = Mode::NewAgent {
            repo_index: 0,
            branch_mode: BranchMode::New,
            prompt: String::new(),
            focus: NewAgentFocus::Agent,
            base_index: 0,
            branches: vec!["main".into()],
            existing_branches: Vec::new(),
            branch_name: "z-0409-1".into(),
            name_pristine: true,
            agent_name: "codex".to_string(),
        };
        let action = app.handle_key(make_key(KeyCode::Right));
        assert!(matches!(action, Some(Action::PickerNext)));
        let action = app.handle_key(make_key(KeyCode::Left));
        assert!(matches!(action, Some(Action::PickerPrev)));
    }

    // --- ActivityCaptured handler tests ---

    #[test]
    fn activity_captured_first_time_stores_hash_without_status_change() {
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.status = AgentStatus::Running;
        a.last_pane_hash = None;
        app.agents = vec![a];

        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0xdead_beef,
            attached_count: 0,
        });

        assert_eq!(app.agents[0].last_pane_hash, Some(0xdead_beef));
        // Status stays Running; first seed must not claim activity.
        assert!(matches!(app.agents[0].status, AgentStatus::Running));
        assert!(!app.agents[0].seen_activity_since_seed);
    }

    #[test]
    fn activity_captured_unchanged_hash_is_noop() {
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.status = AgentStatus::Running;
        a.last_pane_hash = Some(0x1234);
        app.agents = vec![a];

        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0x1234,
            attached_count: 0,
        });

        assert_eq!(app.agents[0].last_pane_hash, Some(0x1234));
        assert!(matches!(app.agents[0].status, AgentStatus::Running));
    }

    #[test]
    fn activity_captured_single_hash_change_is_tentative() {
        // EMIT_THRESHOLD = 2: a single hash change resets quiet_captures
        // and increments consecutive_emits, but does NOT yet flip
        // seen_activity_since_seed. This filters out one-frame blips
        // (cursor blinks, terminal title rewrites) that would otherwise
        // resurrect a finished agent's spinner.
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.status = AgentStatus::Running;
        a.last_pane_hash = Some(0x1234);
        a.quiet_captures = 5;
        a.seen_activity_since_seed = false;
        app.agents = vec![a];

        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0x5678,
            attached_count: 0,
        });

        assert_eq!(app.agents[0].last_pane_hash, Some(0x5678));
        assert_eq!(app.agents[0].quiet_captures, 0);
        assert_eq!(app.agents[0].consecutive_emits, 1);
        assert!(
            !app.agents[0].seen_activity_since_seed,
            "single hash change must not yet claim activity"
        );
        assert!(app.dirty);
    }

    #[test]
    fn activity_captured_two_consecutive_hash_changes_mark_activity() {
        // After EMIT_THRESHOLD consecutive hash-change captures, the
        // activity claim sticks: shows_spinner can flip on, and the
        // notification edge becomes possible when the agent later quiets.
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.status = AgentStatus::Running;
        a.last_pane_hash = Some(0x1234);
        a.seen_activity_since_seed = false;
        app.agents = vec![a];

        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0x5678,
            attached_count: 0,
        });
        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0x9abc,
            attached_count: 0,
        });

        assert_eq!(app.agents[0].consecutive_emits, 2);
        assert!(app.agents[0].seen_activity_since_seed);
    }

    #[test]
    fn activity_captured_isolates_per_agent() {
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        let mut b = mock_agent("refactor");
        a.last_pane_hash = Some(0x1111);
        b.last_pane_hash = Some(0x2222);
        a.status = AgentStatus::Running;
        b.status = AgentStatus::Running;
        b.quiet_captures = 4;
        b.seen_activity_since_seed = true;
        app.agents = vec![a, b];

        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0x9999,
            attached_count: 0,
        });

        assert_eq!(app.agents[0].last_pane_hash, Some(0x9999));
        assert_eq!(app.agents[1].last_pane_hash, Some(0x2222));
        // Other agent's observation fields are untouched.
        assert_eq!(app.agents[1].quiet_captures, 4);
        assert!(app.agents[1].seen_activity_since_seed);
    }

    #[test]
    fn activity_captured_reseeds_when_attach_count_changes() {
        // Attaching to a tmux session resizes its pane to the client's
        // geometry, reflowing wrapped lines — capture-pane output then differs
        // even when the agent emitted nothing. The handler must reseed the
        // hash without claiming activity when attached_count changes.
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.status = AgentStatus::Running;
        a.last_pane_hash = Some(0x1234);
        a.last_attached_count = Some(0);
        a.seen_activity_since_seed = false;
        app.agents = vec![a];

        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0x5678, // changed due to reflow
            attached_count: 1,    // user just attached
        });

        assert_eq!(app.agents[0].last_pane_hash, Some(0x5678));
        assert_eq!(app.agents[0].last_attached_count, Some(1));
        assert!(
            !app.agents[0].seen_activity_since_seed,
            "attach-induced hash change must not claim activity"
        );
    }

    #[test]
    fn activity_captured_after_detach_reseed_does_not_mark_activity() {
        // Real attach/detach can't be guarded by the attached_count delta
        // alone: events.stop() halts Tick during attach, so no capture
        // observes ac=1, and the 0→1→0 round-trip looks like a stable ac=0
        // with a changed hash (pane reflow on detach). The detach path in
        // main::suspend_and_attach compensates by clearing last_pane_hash to
        // None. Verify that with that reseed in place, the first post-detach
        // ActivityCaptured seeds the hash without marking activity —
        // preventing a spurious spinner flicker after the user detaches.
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.status = AgentStatus::Running;
        a.last_pane_hash = None; // cleared on detach by suspend_and_attach
        a.last_attached_count = Some(0); // unchanged across the unobserved attach
        a.quiet_captures = 4;
        a.seen_activity_since_seed = false;
        app.agents = vec![a];

        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0xc0ffee, // pane reflowed on detach — hash differs
            attached_count: 0,
        });

        assert_eq!(app.agents[0].last_pane_hash, Some(0xc0ffee));
        // First post-detach capture takes the None-hash branch: it reseeds
        // and zeroes the counter without claiming activity.
        assert_eq!(app.agents[0].quiet_captures, 0);
        assert!(
            !app.agents[0].seen_activity_since_seed,
            "post-detach reseed must not mark activity"
        );
    }

    #[test]
    fn activity_captured_first_seed_zeros_counter_without_marking_activity() {
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.last_pane_hash = None;
        a.quiet_captures = 99;
        a.seen_activity_since_seed = false;
        app.agents = vec![a];

        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0xc0ffee,
            attached_count: 0,
        });

        assert_eq!(app.agents[0].quiet_captures, 0);
        assert!(
            !app.agents[0].seen_activity_since_seed,
            "first seed must not claim activity"
        );
    }

    #[test]
    fn activity_captured_unchanged_hash_increments_quiet_captures() {
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.last_pane_hash = Some(0xc0ffee);
        a.quiet_captures = 3;
        a.seen_activity_since_seed = true;
        app.agents = vec![a];

        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0xc0ffee,
            attached_count: 0,
        });

        assert_eq!(app.agents[0].quiet_captures, 4);
        assert!(app.agents[0].seen_activity_since_seed);
    }

    #[test]
    fn activity_captured_attach_changed_reseeds_without_marking_activity() {
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.last_pane_hash = Some(0xc0ffee);
        a.last_attached_count = Some(0);
        a.quiet_captures = 5;
        a.seen_activity_since_seed = false;
        app.agents = vec![a];

        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0xfeedbeef,
            attached_count: 1, // user attached — pane reflowed
        });

        assert_eq!(
            app.agents[0].quiet_captures, 0,
            "reseed must reset counter so we don't accumulate against a stale hash"
        );
        assert!(
            !app.agents[0].seen_activity_since_seed,
            "reflow on attach is not real activity"
        );
    }

    #[test]
    fn on_session_detached_clears_pane_state() {
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.status = AgentStatus::Running;
        a.last_pane_hash = Some(0xdeadbeef);
        a.quiet_captures = 4;
        a.seen_activity_since_seed = true;
        app.agents = vec![a];

        app.on_session_detached("z-myapp-fix-auth");

        assert_eq!(app.agents[0].last_pane_hash, None);
        assert_eq!(app.agents[0].quiet_captures, 0);
        assert!(
            app.agents[0].seen_activity_since_seed,
            "lifetime activity history is preserved across detach"
        );
        assert!(
            matches!(app.agents[0].status, AgentStatus::Running),
            "status untouched on detach"
        );
    }

    #[test]
    fn activity_captured_bumps_when_hash_changes_with_stable_attach_count() {
        // Sanity: with attached_count unchanged, a sustained burst of
        // hash deltas (>= EMIT_THRESHOLD) is real activity.
        let mut app = test_app();
        let mut a = mock_agent("fix-auth");
        a.status = AgentStatus::Running;
        a.last_pane_hash = Some(0x1234);
        a.last_attached_count = Some(1);
        a.quiet_captures = 5;
        a.seen_activity_since_seed = false;
        app.agents = vec![a];

        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0x5678,
            attached_count: 1,
        });
        app.update(Action::ActivityCaptured {
            session_name: "z-myapp-fix-auth".into(),
            content: None,
            content_hash: 0x9abc,
            attached_count: 1,
        });

        assert!(
            app.agents[0].seen_activity_since_seed,
            "two consecutive hash deltas with stable attach count is genuine activity"
        );
        assert_eq!(app.agents[0].quiet_captures, 0);
    }

    #[test]
    fn tick_schedules_capture_activity_for_each_session_agent() {
        let mut app = test_app();
        let active_a = mock_agent("a");
        let active_b = mock_agent("b");
        let mut stopped = mock_agent("c");
        stopped.status = AgentStatus::Stopped;
        app.agents = vec![active_a, active_b, stopped];

        // Advance to a tick where the every-5 scheduler fires.
        let mut last_cmds = vec![];
        for _ in 0..5 {
            last_cmds = app.update(Action::Tick);
        }

        let captures: Vec<&str> = last_cmds
            .iter()
            .filter_map(|c| match c {
                Command::CaptureActivity { session_name } => Some(session_name.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(
            captures.len(),
            2,
            "expected 2 capture cmds, got {captures:?}"
        );
        assert!(captures.contains(&"z-myapp-a"));
        assert!(captures.contains(&"z-myapp-b"));
        assert!(!captures.contains(&"z-myapp-c"));
    }

    #[test]
    fn agents_refreshed_preserves_active_status_and_hash() {
        let mut app = test_app();
        // Existing agent with content-hash tracking in flight.
        let mut existing = mock_agent("fix-auth");
        existing.status = AgentStatus::Running;
        existing.last_pane_hash = Some(0xfeed_face);
        existing.last_attached_count = Some(1);
        app.agents = vec![existing];

        // Discover returns a fresh agent with default observation fields.
        // We expect the in-memory state to win.
        let mut from_discover = mock_agent("fix-auth");
        from_discover.status = AgentStatus::Running;
        from_discover.last_pane_hash = None;
        from_discover.last_attached_count = None;
        app.update(Action::AgentsRefreshed(vec![from_discover]));

        assert!(matches!(app.agents[0].status, AgentStatus::Running));
        assert_eq!(app.agents[0].last_pane_hash, Some(0xfeed_face));
        assert_eq!(app.agents[0].last_attached_count, Some(1));
    }

    #[test]
    fn agents_refreshed_preserves_observation_fields() {
        let mut app = test_app();
        let mut existing = mock_agent("fix-auth");
        existing.status = AgentStatus::Running;
        existing.last_pane_hash = Some(0xfeed_face);
        existing.last_attached_count = Some(1);
        existing.quiet_captures = 4;
        existing.seen_activity_since_seed = true;
        existing.was_spinner_visible = true;
        app.agents = vec![existing];

        let mut from_discover = mock_agent("fix-auth");
        from_discover.status = AgentStatus::Running;
        from_discover.last_pane_hash = None;
        from_discover.last_attached_count = None;
        from_discover.quiet_captures = 0;
        from_discover.seen_activity_since_seed = false;
        from_discover.was_spinner_visible = false;
        app.update(Action::AgentsRefreshed(vec![from_discover]));

        assert_eq!(app.agents[0].quiet_captures, 4);
        assert!(app.agents[0].seen_activity_since_seed);
        assert!(app.agents[0].was_spinner_visible);
    }

    #[test]
    fn agents_refreshed_uses_new_status_when_old_was_not_active() {
        let mut app = test_app();
        let mut existing = mock_agent("fix-auth");
        existing.status = AgentStatus::Stopped;
        existing.last_pane_hash = None;
        app.agents = vec![existing];

        // Discover finds a session for a previously-stopped agent.
        let mut from_discover = mock_agent("fix-auth");
        from_discover.status = AgentStatus::Running;
        from_discover.last_pane_hash = None;
        app.update(Action::AgentsRefreshed(vec![from_discover]));

        // New status (Running) wins.
        assert!(matches!(app.agents[0].status, AgentStatus::Running));
    }

    #[test]
    fn newagent_branchlist_k_moves_up() {
        let mut app = test_app_in_new_agent_mode();
        if let Mode::NewAgent { focus, .. } = &mut app.mode {
            *focus = NewAgentFocus::BranchList;
        }
        let action = app.handle_key(make_key(KeyCode::Char('k')));
        assert!(matches!(action, Some(Action::PickerPrev)));
    }

    #[test]
    fn newagent_branchlist_j_moves_down() {
        let mut app = test_app_in_new_agent_mode();
        if let Mode::NewAgent { focus, .. } = &mut app.mode {
            *focus = NewAgentFocus::BranchList;
        }
        let action = app.handle_key(make_key(KeyCode::Char('j')));
        assert!(matches!(action, Some(Action::PickerNext)));
    }

    #[test]
    fn newagent_prompt_j_still_types() {
        let app = test_app_in_new_agent_mode();
        let action = app.handle_key(make_key(KeyCode::Char('j')));
        assert!(matches!(action, Some(Action::TypeChar('j'))));
    }

    #[test]
    fn confirmdelete_q_cancels() {
        let mut app = test_app();
        app.agents = vec![mock_agent("a")];
        app.update(Action::StartDelete);
        let action = app.handle_key(make_key(KeyCode::Char('q')));
        assert!(matches!(action, Some(Action::CancelMode)));
    }

    #[test]
    fn newagent_branchlist_q_cancels() {
        let mut app = test_app_in_new_agent_mode();
        if let Mode::NewAgent { focus, .. } = &mut app.mode {
            *focus = NewAgentFocus::BranchList;
        }
        let action = app.handle_key(make_key(KeyCode::Char('q')));
        assert!(matches!(action, Some(Action::CancelMode)));
    }

    #[test]
    fn newagent_prompt_q_still_types() {
        let app = test_app_in_new_agent_mode();
        let action = app.handle_key(make_key(KeyCode::Char('q')));
        assert!(matches!(action, Some(Action::TypeChar('q'))));
    }

    #[test]
    fn newagent_name_q_still_types() {
        let mut app = test_app_in_new_agent_mode();
        if let Mode::NewAgent { focus, .. } = &mut app.mode {
            *focus = NewAgentFocus::Name;
        }
        let action = app.handle_key(make_key(KeyCode::Char('q')));
        assert!(matches!(action, Some(Action::TypeChar('q'))));
    }

    #[test]
    fn refresh_agents_also_requests_mr_refresh() {
        let mut app = test_app();
        let cmds = app.update(Action::RefreshAgents);
        assert!(cmds.iter().any(|c| matches!(c, Command::Discover(_))));
    }

    #[test]
    fn agents_refreshed_requests_one_mr_refresh_per_agent() {
        let mut app = test_app();
        let cmds = app.update(Action::AgentsRefreshed(vec![
            mock_agent("fix-auth"),
            mock_agent("docs"),
        ]));
        let count = cmds
            .iter()
            .filter(|c| matches!(c, Command::RefreshMr { .. }))
            .count();
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
        app.mr_snapshots
            .insert(key, MrSnapshot::Ready(test_mr("fix-auth")));
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
        assert_eq!(
            app.status_message.as_deref(),
            Some("not ready; use f make-ready")
        );
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
        assert_eq!(
            app.status_message.as_deref(),
            Some("agent running; attach or stop first")
        );
    }

    #[test]
    fn stopped_agent_intent_starts_session() {
        let mut app = test_app();
        let mut agent = mock_agent("fix-auth");
        agent.status = AgentStatus::Stopped;
        app.agents = vec![agent];
        let cmds = app.update(Action::MrIntent(MrIntent::Rebase));
        assert!(
            matches!(cmds.as_slice(), [Command::StartAgentIntent { fresh_cmd, .. }] if fresh_cmd.contains("Rebase this worktree"))
        );
    }

    #[test]
    fn agentic_rebase_uses_mr_target_branch_when_base_missing() {
        let mut app = test_app();
        let mut agent = mock_agent("fix-auth");
        agent.status = AgentStatus::Stopped;
        agent.base_branch = None;
        app.agents = vec![agent];
        let key = app.selected_mr_key().unwrap();
        let mut mr = test_mr("fix-auth");
        mr.target_branch = Some("develop".into());
        app.mr_snapshots.insert(key, MrSnapshot::Ready(mr));

        let cmds = app.update(Action::MrIntent(MrIntent::Rebase));

        assert!(matches!(
            cmds.as_slice(),
            [Command::StartAgentIntent { fresh_cmd, .. }]
                if fresh_cmd.contains("onto develop")
        ));
    }

    #[test]
    fn refresh_mrs_with_no_agents_does_not_get_stuck() {
        let mut app = test_app();
        let cmds = app.schedule_mr_refresh();
        assert!(cmds.is_empty());

        app.agents = vec![mock_agent("fix-auth")];
        let cmds = app.schedule_mr_refresh();
        assert_eq!(
            cmds.iter()
                .filter(|c| matches!(c, Command::RefreshMr { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn mr_refresh_stays_pending_until_batch_completes() {
        let mut app = test_app();
        app.agents = vec![mock_agent("fix-auth"), mock_agent("docs")];
        let cmds = app.schedule_mr_refresh();
        assert_eq!(cmds.len(), 2);

        let first_key = MrKey::new("/tmp/repo".into(), "fix-auth".into());
        let second_key = MrKey::new("/tmp/repo".into(), "docs".into());
        app.update(Action::MrRefreshed {
            key: first_key,
            snapshot: MrSnapshot::Missing,
        });
        let cmds = app.schedule_mr_refresh();
        assert!(cmds.is_empty());

        app.update(Action::MrRefreshed {
            key: second_key,
            snapshot: MrSnapshot::Missing,
        });
        let cmds = app.schedule_mr_refresh();
        assert_eq!(
            cmds.iter()
                .filter(|c| matches!(c, Command::RefreshMr { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn make_ready_without_mr_is_refused() {
        let mut app = test_app();
        let mut agent = mock_agent("fix-auth");
        agent.status = AgentStatus::Stopped;
        app.agents = vec![agent];
        let cmds = app.update(Action::MrIntent(MrIntent::MakeReady));
        assert!(cmds.is_empty());
        assert_eq!(app.status_message.as_deref(), Some("no MR"));
    }

    #[test]
    fn review_fix_without_mr_is_refused() {
        let mut app = test_app();
        let mut agent = mock_agent("fix-auth");
        agent.status = AgentStatus::Stopped;
        app.agents = vec![agent];
        let cmds = app.update(Action::MrIntent(MrIntent::ReviewFix));
        assert!(cmds.is_empty());
        assert_eq!(app.status_message.as_deref(), Some("no MR"));
    }

    #[test]
    fn open_mr_command_carries_repo_key() {
        let mut app = test_app();
        app.agents = vec![mock_agent("fix-auth")];
        let key = app.selected_mr_key().unwrap();
        app.mr_snapshots
            .insert(key.clone(), MrSnapshot::Ready(test_mr("fix-auth")));
        let cmds = app.update(Action::MrOpen);
        assert!(matches!(
            cmds.as_slice(),
            [Command::OpenMr { key: command_key, id_or_branch }]
                if command_key == &key && id_or_branch == "1"
        ));
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
}
