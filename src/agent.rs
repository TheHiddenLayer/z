use std::path::{Path, PathBuf};

// --- Data types ---

#[derive(Debug, Clone, PartialEq)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: String,
    pub branch: Option<String>,
    pub is_main: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TmuxSession {
    pub name: String,
    pub path: String,
}

/// Number of consecutive same-hash captures required to declare an agent
/// idle. At 500ms/capture this is ~3.5s of *observed* quiet. The counter
/// only advances on actual captures, so event-loop gaps (tmux attach,
/// OS suspend, lid close) stall it rather than expiring a deadline —
/// they cannot fire spurious "agent finished" notifications.
pub const QUIET_THRESHOLD: u32 = 7;

/// Number of consecutive captures with hash *changes* required to confirm
/// real activity. A single hash change is tentative — it could be a
/// one-frame blip (cursor blink, terminal title rewrite, a stray escape
/// sequence after the agent has otherwise finished). Requiring two
/// consecutive emits filters these out without delaying the spinner
/// noticeably for genuine work, which produces dozens of changes in a row.
pub const EMIT_THRESHOLD: u32 = 2;

#[derive(Debug, Clone, PartialEq)]
pub enum AgentStatus {
    Creating,
    Running,
    Stopped,
    Error(String),
}

impl AgentStatus {
    pub fn has_session(&self) -> bool {
        matches!(self, AgentStatus::Running)
    }
}

#[derive(Debug, Clone)]
pub struct Agent {
    pub repo_path: PathBuf,
    pub repo_name: String,
    /// The worktree's *current* branch — a live readout from git that can
    /// change if someone runs `git checkout` inside the worktree.
    pub branch: String,
    pub base_branch: Option<String>,
    pub worktree_path: PathBuf,
    /// Stable identity derived from the worktree path, with slashes flattened.
    /// Equal to `branch.replace('/', '-')` at creation time; diverges from
    /// `branch` after a `git checkout` inside the worktree.
    pub slug: String,
    pub session_name: String,
    pub status: AgentStatus,
    /// The agent name stored in `z.agent` for this worktree. Surfaced verbatim
    /// in the UI even when not present in the current config.
    pub agent_name: String,
    /// Last observed hash of the tmux pane content. Compared on each capture
    /// to detect output activity. `None` until the first capture lands.
    pub last_pane_hash: Option<u64>,
    /// Last observed `#{session_attached}` (count of clients attached to the
    /// tmux session). When this changes, tmux resizes the pane to the new
    /// client's geometry and reflows wrapped lines — capture-pane output then
    /// differs even when the agent emitted no new bytes. We reseed the hash
    /// on attach-count changes to suppress those false positives.
    pub last_attached_count: Option<u32>,
    /// Consecutive `ActivityCaptured` observations with `last_pane_hash`
    /// unchanged. Resets to 0 on a real hash delta, on the initial seed,
    /// and on the post-detach reseed (where we can't tell reflow from
    /// real activity). Reaching `QUIET_THRESHOLD` flips `shows_spinner()`
    /// to false.
    pub quiet_captures: u32,
    /// Set true the first time we observe a hash delta on an agent that
    /// already has a seed (i.e. genuine activity, not the initial sample
    /// or a post-detach reflow). Required to suppress a spurious
    /// "finished" notification on agents that were already idle when z
    /// first discovered them — without it, the natural `working → idle`
    /// edge fires on every freshly-discovered idle session.
    pub seen_activity_since_seed: bool,
    /// `shows_spinner()` from the previous Tick. Used in `Action::Tick`
    /// to detect the per-agent spinner→done edge that fires desktop
    /// notifications. Per-agent (rather than a global HashSet) so the
    /// edge survives any pause in the Tick loop without stale state.
    pub was_spinner_visible: bool,
    /// Consecutive captures with hash changes (each capture's hash differs
    /// from the previous). Resets to 0 on any same-hash capture. Used to
    /// confirm real activity (`>= EMIT_THRESHOLD`) before flipping
    /// `seen_activity_since_seed = true`, filtering out single-capture
    /// blips that would otherwise resurrect the spinner and re-fire a
    /// "done" notification after one already fired.
    pub consecutive_emits: u32,
}

impl Agent {
    /// True iff the UI should render a spinner for this agent.
    ///
    /// Three regimes:
    /// - `(true, Some(_))`: we've observed a real hash delta since the last
    ///   seed and still have a hash — straightforward time-based hysteresis
    ///   on `quiet_captures`.
    /// - `(false, Some(_))`: hash is seeded but no real activity has been
    ///   observed in this window — fall back to `was_spinner_visible` (so
    ///   a freshly-discovered idle agent stays idle, while a working agent
    ///   that just had its hash reseeded by attach/detach keeps its spinner
    ///   until the next capture confirms otherwise). Bounded by
    ///   `quiet_captures < QUIET_THRESHOLD` so the spinner doesn't get
    ///   stuck on after a detach if the agent silently went idle.
    /// - `(_, None)`: hash cleared by detach, no observation yet — show
    ///   whatever we showed last tick.
    ///
    /// The notification edge in `Action::Tick` gates additionally on
    /// `seen_activity_since_seed` so the spinner-may-flicker-off transition
    /// in the middle case never fires a "agent finished" notification — it
    /// only fires on the genuine `(true, Some)` → `quiet >= threshold` edge.
    pub fn shows_spinner(&self) -> bool {
        match self.status {
            AgentStatus::Creating => true,
            AgentStatus::Running => {
                match (self.seen_activity_since_seed, self.last_pane_hash) {
                    (true, Some(_)) => self.quiet_captures < QUIET_THRESHOLD,
                    (false, Some(_)) => {
                        self.was_spinner_visible
                            && self.quiet_captures < QUIET_THRESHOLD
                    }
                    (_, None) => self.was_spinner_visible,
                }
            }
            AgentStatus::Stopped | AgentStatus::Error(_) => false,
        }
    }
}

// --- Pure parsing functions (unchanged logic) ---

const TMUX_PREFIX: &str = "z";

pub fn session_name(repo_basename: &str, branch: &str) -> String {
    let sanitized_branch = branch.replace('/', "-");
    format!("{TMUX_PREFIX}-{repo_basename}-{sanitized_branch}")
}

/// Slug derived from the worktree path relative to `<repo>-worktrees/`, with
/// slashes flattened. Returns None if the worktree lives outside that base —
/// in which case callers should fall back to branch-derived naming.
///
/// The worktree path is the agent's stable identity: `git checkout` inside an
/// agent worktree changes its current branch but not its path, so deriving the
/// session name from the path keeps tmux lookups stable across branch drift.
fn worktree_slug(repo_path: &Path, worktree_path: &Path) -> Option<String> {
    let repo_name = repo_path.file_name()?.to_str()?;
    let worktrees_base = repo_path.parent()?.join(format!("{repo_name}-worktrees"));
    let rel = worktree_path.strip_prefix(&worktrees_base).ok()?;
    Some(rel.to_string_lossy().replace('/', "-"))
}

pub fn parse_worktree_list(output: &str) -> Vec<Worktree> {
    let mut worktrees = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_head = String::new();
    let mut current_branch: Option<String> = None;
    let mut is_first = true;

    for line in output.lines() {
        if line.is_empty() {
            if let Some(path) = current_path.take() {
                worktrees.push(Worktree {
                    path,
                    head: std::mem::take(&mut current_head),
                    branch: current_branch.take(),
                    is_main: is_first,
                });
                is_first = false;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("worktree ") {
            current_path = Some(PathBuf::from(rest));
        } else if let Some(rest) = line.strip_prefix("HEAD ") {
            current_head = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("branch refs/heads/") {
            current_branch = Some(rest.to_string());
        }
    }

    if let Some(path) = current_path.take() {
        worktrees.push(Worktree {
            path,
            head: current_head,
            branch: current_branch,
            is_main: is_first,
        });
    }

    worktrees
}

pub fn parse_session_list(output: &str) -> Vec<TmuxSession> {
    use std::collections::HashMap;

    let mut sessions: HashMap<String, TmuxSession> = HashMap::new();

    for line in output.lines().filter(|l| !l.is_empty()) {
        let mut parts = line.splitn(2, '\t');
        let name = match parts.next() {
            Some(n) if n.starts_with(&format!("{TMUX_PREFIX}-")) => n.to_string(),
            _ => continue,
        };
        let path = parts.next().unwrap_or("").to_string();

        sessions
            .entry(name.clone())
            .or_insert(TmuxSession { name, path });
    }

    sessions.into_values().collect()
}

/// Build agents from a list of (worktree, agent_name) pairs. Caller is
/// responsible for filtering out non-z-managed worktrees (those without
/// `z.agent` set) before invoking. Main worktrees are still skipped here as
/// a safety net.
pub fn discover_agents(
    repo_path: &Path,
    entries: &[(Worktree, String)],
    sessions: &[TmuxSession],
) -> Vec<Agent> {
    let repo_name = repo_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    entries
        .iter()
        .filter(|(wt, _)| !wt.is_main)
        .map(|(wt, agent_name)| {
            let branch = wt.branch.as_deref().unwrap_or("detached");
            let slug = worktree_slug(repo_path, &wt.path)
                .unwrap_or_else(|| branch.replace('/', "-"));
            let sess_name = format!("{TMUX_PREFIX}-{repo_name}-{slug}");
            let session = sessions.iter().find(|s| s.name == sess_name);

            Agent {
                repo_path: repo_path.to_path_buf(),
                repo_name: repo_name.clone(),
                branch: branch.to_string(),
                base_branch: None,
                worktree_path: wt.path.clone(),
                slug,
                session_name: sess_name,
                status: match session {
                    // Seed Running: shows_spinner() handles initial freshness
                    // via last_pane_hash being None until the first capture
                    // lands. Avoids relying on tmux's coarse window_activity.
                    Some(_) => AgentStatus::Running,
                    None => AgentStatus::Stopped,
                },
                agent_name: agent_name.clone(),
                last_pane_hash: None,
                last_attached_count: None,
                quiet_captures: 0,
                seen_activity_since_seed: false,
                was_spinner_visible: false,
                consecutive_emits: 0,
            }
        })
        .collect()
}

// --- Async command wrappers ---

use tokio::process::Command;

pub async fn list_worktrees(repo_path: &Path) -> Result<Vec<Worktree>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .await
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git worktree list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(parse_worktree_list(&String::from_utf8_lossy(&output.stdout)))
}

pub async fn fetch_origin(repo_path: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["fetch", "origin"])
        .output()
        .await
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git fetch failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

pub async fn list_branches(repo_path: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args([
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads/",
            "refs/remotes/origin/",
        ])
        .output()
        .await
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git branch list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let name = if let Some(local) = line.strip_prefix("refs/heads/") {
            local.to_string()
        } else if let Some(remote) = line.strip_prefix("refs/remotes/origin/") {
            if remote == "HEAD" {
                continue;
            }
            remote.to_string()
        } else {
            continue;
        };
        seen.insert(name);
    }
    Ok(seen.into_iter().collect())
}

pub async fn list_sessions() -> Vec<TmuxSession> {
    // We only need session existence + path here. Activity is driven by
    // capture-pane content-hash polling via shows_spinner()'s observation
    // counters; window_activity's 1s granularity is too coarse.
    let output = Command::new("tmux")
        .args(["list-windows", "-a", "-F", "#{session_name}\t#{session_path}"])
        .output()
        .await;
    match output {
        Ok(out) if out.status.success() => {
            parse_session_list(&String::from_utf8_lossy(&out.stdout))
        }
        _ => Vec::new(),
    }
}

pub async fn create_worktree(
    repo_path: &Path,
    branch: &str,
    new_branch: bool,
    base_branch: Option<&str>,
    agent_name: &str,
) -> Result<PathBuf, String> {
    // Validate branch name against git's own rules. Run inside repo so
    // `--branch` can resolve `@{-N}` shorthand (it errors outside a worktree).
    let check = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["check-ref-format", "--branch", branch])
        .output()
        .await
        .map_err(|e| format!("git failed: {e}"))?;
    if !check.status.success() {
        let stderr = String::from_utf8_lossy(&check.stderr);
        return Err(format!(
            "invalid branch name: {branch} ({})",
            stderr.trim()
        ));
    }

    // Refuse to create a branch that already exists
    if new_branch {
        let exists = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["rev-parse", "--verify", &format!("refs/heads/{branch}")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map_err(|e| format!("git failed: {e}"))?;
        if exists.success() {
            return Err(format!("branch '{branch}' already exists"));
        }
    }

    let repo_name = repo_path
        .file_name()
        .ok_or("invalid repo path")?
        .to_str()
        .ok_or("non-utf8 repo name")?;
    let worktree_base = repo_path
        .parent()
        .ok_or("repo has no parent")?
        .join(format!("{repo_name}-worktrees"));
    let worktree_path = worktree_base.join(branch);

    tokio::fs::create_dir_all(&worktree_base)
        .await
        .map_err(|e| format!("mkdir failed: {e}"))?;

    let wt_str = worktree_path.to_str().ok_or("non-utf8 path")?;
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo_path);
    if new_branch {
        cmd.args(["worktree", "add", wt_str, "-b", branch]);
        if let Some(base) = base_branch {
            // Prefer the remote tracking ref for freshest upstream state.
            // Falls back to the local branch if no remote counterpart exists.
            let remote_ref = format!("origin/{base}");
            let has_remote = Command::new("git")
                .arg("-C")
                .arg(repo_path)
                .args(["rev-parse", "--verify", &format!("refs/remotes/{remote_ref}")])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await
                .map(|s| s.success())
                .unwrap_or(false);
            if has_remote {
                cmd.arg(&remote_ref);
            } else {
                cmd.arg(base);
            }
        }
    } else {
        cmd.args(["worktree", "add", wt_str, branch]);
    }

    let output = cmd.output().await.map_err(|e| format!("git failed: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git worktree add failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Enable worktree-scoped config on the main repo (idempotent), then write
    // agent metadata into the worktree's own config. Keying by worktree (not
    // branch) means a `git checkout` inside the worktree doesn't orphan the
    // metadata.
    let _ = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["config", "extensions.worktreeConfig", "true"])
        .output()
        .await;

    if let Some(base) = base_branch {
        let _ = Command::new("git")
            .arg("-C")
            .arg(&worktree_path)
            .args(["config", "--worktree", "z.base", base])
            .output()
            .await;
    }

    let _ = Command::new("git")
        .arg("-C")
        .arg(&worktree_path)
        .args(["config", "--worktree", "z.agent", agent_name])
        .output()
        .await;

    Ok(worktree_path)
}

pub async fn delete_branch(repo_path: &Path, branch: &str) -> Result<(), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["branch", "-D", branch])
        .output()
        .await
        .map_err(|e| format!("git failed: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git branch -D failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

pub async fn remove_worktree(repo_path: &Path, worktree_path: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args([
            "worktree",
            "remove",
            "--force",
            worktree_path.to_str().ok_or("non-utf8 path")?,
        ])
        .output()
        .await
        .map_err(|e| format!("git failed: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git worktree remove failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

pub async fn create_session(name: &str, working_dir: &Path, command: Option<&str>) -> Result<(), String> {
    let dir_str = working_dir.to_str().ok_or("non-utf8 path")?;
    let mut cmd = Command::new("tmux");
    cmd.args(["set-option", "-g", "history-limit", "50000", ";",
              "new-session", "-d", "-s", name, "-c", dir_str]);
    let output = cmd.output().await.map_err(|e| format!("tmux failed: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "tmux new-session failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Keep the tmux session alive after the agent process exits.
    if let Some(shell_command) = command {
        if let Err(e) = send_shell_command(name, shell_command).await {
            let _ = kill_session(name).await;
            return Err(e);
        }
    }

    Ok(())
}

async fn send_shell_command(session: &str, shell_command: &str) -> Result<(), String> {
    let output = Command::new("tmux")
        .args(["send-keys", "-t", session, "-l", shell_command])
        .output()
        .await
        .map_err(|e| format!("tmux failed: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "tmux send-keys failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let output = Command::new("tmux")
        .args(["send-keys", "-t", session, "Enter"])
        .output()
        .await
        .map_err(|e| format!("tmux failed: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "tmux send-keys failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

pub async fn kill_session(name: &str) -> Result<(), String> {
    let output = Command::new("tmux")
        .args(["kill-session", "-t", name])
        .output()
        .await
        .map_err(|e| format!("tmux failed: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "tmux kill-session failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

/// Number of clients currently attached to the tmux session. Used to detect
/// attach/detach events between activity polls so we can suppress the false
/// "active" signal that pane reflow produces.
pub async fn session_attached_count(session: &str) -> Option<u32> {
    let output = Command::new("tmux")
        .args(["display-message", "-p", "-t", session, "#{session_attached}"])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

pub async fn capture_pane(session: &str) -> Option<String> {
    // -S -200 caps the upper bound at 200 lines of history. tmux returns
    // fewer when the buffer is shorter, so this is a ceiling, not a floor —
    // sparse sessions still return only what's there. The UI tails the
    // result down to the preview area's height.
    let output = Command::new("tmux")
        .args(["capture-pane", "-t", session, "-p", "-S", "-200"])
        .output()
        .await
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}

/// If we're running inside tmux, enable focus-event passthrough server-wide
/// so the outer tmux forwards FocusGained / FocusLost into our pty. Idempotent.
///
/// Caveat (per the tmux manpage): on a tmux server that has never had this
/// option set, attached clients must detach + reattach before focus events
/// start flowing. Once set, subsequent z runs in the same server work
/// immediately.
pub fn enable_tmux_focus_events() {
    if std::env::var_os("TMUX").is_none() { return; }
    let _ = std::process::Command::new("tmux")
        .args(["set-option", "-g", "focus-events", "on"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Blocking attach — intentionally synchronous. Hands terminal to tmux.
pub fn attach(name: &str) -> Result<(), String> {
    let status = std::process::Command::new("tmux")
        .args(["attach-session", "-t", name])
        .status()
        .map_err(|e| format!("tmux failed: {e}"))?;
    if !status.success() {
        return Err("tmux attach failed".into());
    }
    Ok(())
}

// --- Async discovery (combines worktree list + session list) ---

/// Read all `z.*` worktree-scoped config keys in one git invocation.
/// Returns `Some((agent_name, base))` iff `z.agent` is set (the marker that
/// this worktree is z-managed). `agent_name` is the raw stored string —
/// surfaced verbatim in the UI even if not present in the current config.
/// `base` is `Some(value)` iff `z.base` is set and non-empty. Returns `None`
/// for worktrees not created by z.
async fn read_z_meta(worktree_path: &Path) -> Option<(String, Option<String>)> {
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree_path)
        .args(["config", "--worktree", "--get-regexp", "^z\\."])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut agent_name: Option<String> = None;
    let mut base: Option<String> = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((key, val)) = line.split_once(' ') else { continue };
        let val = val.trim();
        match key {
            "z.agent" => agent_name = Some(val.to_string()),
            "z.base" if !val.is_empty() => base = Some(val.to_string()),
            _ => {}
        }
    }
    agent_name.map(|n| (n, base))
}

pub async fn discover_all(repos: &[PathBuf]) -> Vec<Agent> {
    // Run session listing and all worktree listings in parallel
    let sessions_fut = list_sessions();
    let worktree_futs: Vec<_> = repos
        .iter()
        .map(|repo| async move { (repo.clone(), list_worktrees(repo).await) })
        .collect();
    let (sessions, worktree_results) = tokio::join!(
        sessions_fut,
        futures::future::join_all(worktree_futs)
    );

    let mut all_agents = Vec::new();
    for (repo_path, result) in worktree_results {
        let Ok(worktrees) = result else { continue };
        // discover_agents skips main worktrees, so filter them now and only
        // pay one git-config invocation per non-main worktree. Each call
        // returns both `z.agent` (z-managed marker) and `z.base` together,
        // halving subprocess cost vs. the previous two-pass design.
        let non_main: Vec<Worktree> = worktrees.into_iter().filter(|wt| !wt.is_main).collect();
        let metas = futures::future::join_all(
            non_main.iter().map(|wt| read_z_meta(&wt.path))
        ).await;
        let triples: Vec<(Worktree, String, Option<String>)> = non_main
            .into_iter()
            .zip(metas)
            .filter_map(|(wt, meta)| meta.map(|(name, base)| (wt, name, base)))
            .collect();
        let entries: Vec<(Worktree, String)> = triples
            .iter()
            .map(|(wt, name, _)| (wt.clone(), name.clone()))
            .collect();
        let mut agents = discover_agents(&repo_path, &entries, &sessions);
        for (agent, (_, _, base)) in agents.iter_mut().zip(triples) {
            agent.base_branch = base;
        }
        all_agents.append(&mut agents);
    }

    all_agents
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // --- AgentStatus tests ---

    #[test]
    fn agent_status_creating_is_not_ready() {
        let status = AgentStatus::Creating;
        assert!(!status.has_session());
    }

    #[test]
    fn agent_status_running_has_session() {
        let status = AgentStatus::Running;
        assert!(status.has_session());
    }

    // --- Worktree parsing (from git.rs) ---

    #[test]
    fn parse_porcelain_output() {
        let output = "\
worktree /home/user/src/myapp
HEAD abc123def456
branch refs/heads/main

worktree /home/user/src/myapp-worktrees/fix-auth
HEAD abc123def456
branch refs/heads/fix-auth
";
        let worktrees = parse_worktree_list(output);
        assert_eq!(worktrees.len(), 2);
        assert_eq!(worktrees[0].path, PathBuf::from("/home/user/src/myapp"));
        assert_eq!(worktrees[0].branch.as_deref(), Some("main"));
        assert!(worktrees[0].is_main);
        assert_eq!(
            worktrees[1].path,
            PathBuf::from("/home/user/src/myapp-worktrees/fix-auth")
        );
        assert_eq!(worktrees[1].branch.as_deref(), Some("fix-auth"));
        assert!(!worktrees[1].is_main);
    }

    #[test]
    fn parse_detached_head() {
        let output = "\
worktree /home/user/src/myapp
HEAD abc123
branch refs/heads/main

worktree /home/user/src/myapp-worktrees/detached
HEAD def456
detached
";
        let worktrees = parse_worktree_list(output);
        assert_eq!(worktrees.len(), 2);
        assert!(worktrees[1].branch.is_none());
    }

    #[test]
    fn parse_empty_output() {
        let worktrees = parse_worktree_list("");
        assert!(worktrees.is_empty());
    }

    // --- Tmux parsing (from tmux.rs) ---

    #[test]
    fn test_session_name() {
        assert_eq!(session_name("myapp", "fix-auth"), "z-myapp-fix-auth");
    }

    #[test]
    fn test_session_name_sanitizes_slashes() {
        assert_eq!(
            session_name("myapp", "feature/auth"),
            "z-myapp-feature-auth"
        );
    }

    #[test]
    fn test_parse_session_list() {
        let output = "z-myapp-fix-auth\t/home/user/src/myapp-worktrees/fix-auth\nother-session\t/tmp\nz-lib-refactor\t/home/user/src/lib-worktrees/refactor\n";
        let sessions = parse_session_list(output);
        assert_eq!(sessions.len(), 2);
        let names: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"z-myapp-fix-auth"));
        assert!(names.contains(&"z-lib-refactor"));
    }

    #[test]
    fn test_parse_session_list_dedups_repeated_sessions() {
        // Multiple windows in one session — should produce one TmuxSession.
        let output = "z-myapp-fix-auth\t/path\nz-myapp-fix-auth\t/path\n";
        let sessions = parse_session_list(output);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "z-myapp-fix-auth");
    }

    #[test]
    fn test_parse_empty_sessions() {
        let sessions = parse_session_list("");
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn create_session_keeps_tmux_alive_after_command_exit() {
        let tmux_available = std::process::Command::new("tmux")
            .arg("-V")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !tmux_available {
            return;
        }

        let name = format!("z-test-persistent-{}", std::process::id());
        let cwd = std::env::current_dir().unwrap();
        let _ = kill_session(&name).await;

        create_session(&name, &cwd, Some("false")).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let found = list_sessions().await.iter().any(|s| s.name == name);
        let _ = kill_session(&name).await;

        assert!(found, "session should remain after its command exits");
    }

    #[tokio::test]
    async fn read_z_meta_distinguishes_z_from_external_worktrees() {
        // Build a temp repo with two worktrees: one tagged with z.agent +
        // z.base (simulating a z-created worktree) and one untagged
        // (simulating an external `git worktree add`).
        let tmp = std::env::temp_dir().join(format!("z-mgmt-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let repo = tmp.join("repo");
        let run = |args: &[&str], cwd: &Path| {
            let status = std::process::Command::new("git")
                .arg("-C").arg(cwd).args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status().unwrap();
            assert!(status.success(), "git {args:?} in {cwd:?}");
        };

        std::fs::create_dir_all(&repo).unwrap();
        run(&["init", "-q", "-b", "main"], &repo);
        run(&["commit", "-q", "--allow-empty", "-m", "init"], &repo);
        run(&["config", "extensions.worktreeConfig", "true"], &repo);

        let z_wt = tmp.join("z-wt");
        let ext_wt = tmp.join("ext-wt");
        run(&["worktree", "add", z_wt.to_str().unwrap(), "-b", "z-branch"], &repo);
        run(&["worktree", "add", ext_wt.to_str().unwrap(), "-b", "ext-branch"], &repo);
        run(&["config", "--worktree", "z.agent", "claude"], &z_wt);
        run(&["config", "--worktree", "z.base", "main"], &z_wt);

        let z_meta = read_z_meta(&z_wt).await;
        assert_eq!(
            z_meta,
            Some(("claude".to_string(), Some("main".to_string()))),
            "z-managed worktree should yield agent name + base",
        );
        assert_eq!(
            read_z_meta(&ext_wt).await,
            None,
            "external worktree without z.agent should yield None",
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // --- Agent discovery ---

    #[test]
    fn merge_worktrees_and_sessions() {
        let repo = PathBuf::from("/home/user/src/myapp");
        let entries = vec![
            (
                Worktree {
                    path: PathBuf::from("/home/user/src/myapp"),
                    head: "abc".into(),
                    branch: Some("main".into()),
                    is_main: true,
                },
                "claude".to_string(),
            ),
            (
                Worktree {
                    path: PathBuf::from("/home/user/src/myapp-worktrees/fix-auth"),
                    head: "def".into(),
                    branch: Some("fix-auth".into()),
                    is_main: false,
                },
                "claude".to_string(),
            ),
            (
                Worktree {
                    path: PathBuf::from("/home/user/src/myapp-worktrees/add-tests"),
                    head: "ghi".into(),
                    branch: Some("add-tests".into()),
                    is_main: false,
                },
                "codex".to_string(),
            ),
        ];
        let sessions = vec![TmuxSession {
            name: "z-myapp-fix-auth".into(),
            path: "/home/user/src/myapp-worktrees/fix-auth".into(),
        }];

        let agents = discover_agents(&repo, &entries, &sessions);
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].branch, "fix-auth");
        // Discover seeds Running; content-hash polling will update later.
        assert_eq!(agents[0].status, AgentStatus::Running);
        assert_eq!(agents[0].agent_name, "claude");
        assert_eq!(agents[1].branch, "add-tests");
        assert_eq!(agents[1].status, AgentStatus::Stopped);
        assert_eq!(agents[1].agent_name, "codex");
    }

    #[test]
    fn agent_tracks_session_after_branch_drift() {
        // Drift case: an agent was created on branch `fix-auth` (worktree at
        // `myapp-worktrees/fix-auth`, tmux session `z-myapp-fix-auth`).
        // Then someone ran `git checkout unrelated-branch` inside that worktree.
        // The live tmux session is still running at the same path — the agent's
        // identity is the worktree, not the current branch — so discovery must
        // still associate the worktree with that session.
        let repo = PathBuf::from("/home/user/src/myapp");
        let entries = vec![(
            Worktree {
                path: PathBuf::from("/home/user/src/myapp-worktrees/fix-auth"),
                head: "def".into(),
                branch: Some("unrelated-branch".into()), // drifted via external checkout
                is_main: false,
            },
            "claude".to_string(),
        )];
        let sessions = vec![TmuxSession {
            name: "z-myapp-fix-auth".into(), // session name reflects the worktree path
            path: "/home/user/src/myapp-worktrees/fix-auth".into(),
        }];

        let agents = discover_agents(&repo, &entries, &sessions);
        assert_eq!(agents.len(), 1);
        // The agent should track the live session, not be reported as Stopped.
        // Discover seeds Running; content-hash polling refreshes this later.
        assert_eq!(agents[0].status, AgentStatus::Running);
        // It should target the live session's name, so attach/kill don't
        // create or miss anything.
        assert_eq!(agents[0].session_name, "z-myapp-fix-auth");
        // Identity (slug) stays anchored to the worktree path, while branch
        // reflects what's currently checked out — this difference is the drift
        // signal the UI surfaces.
        assert_eq!(agents[0].slug, "fix-auth");
        assert_eq!(agents[0].branch, "unrelated-branch");
    }

    #[test]
    fn agent_default_observation_fields_are_zeroed() {
        let a = Agent {
            repo_path: std::path::PathBuf::from("/repo"),
            repo_name: "repo".into(),
            branch: "b".into(),
            base_branch: None,
            worktree_path: std::path::PathBuf::from("/repo/wt"),
            slug: "b".into(),
            session_name: "z-repo-b".into(),
            status: AgentStatus::Running,
            agent_name: "claude".into(),
            last_pane_hash: None,
            last_attached_count: None,
            quiet_captures: 0,
            seen_activity_since_seed: false,
            was_spinner_visible: false,
                consecutive_emits: 0,
        };
        assert_eq!(a.quiet_captures, 0);
        assert!(!a.seen_activity_since_seed);
        assert!(!a.was_spinner_visible);
    }

    #[test]
    fn quiet_threshold_is_seven_captures() {
        assert_eq!(QUIET_THRESHOLD, 7);
    }

    pub(crate) fn make_agent_with_status(status: AgentStatus) -> Agent {
        Agent {
            repo_path: std::path::PathBuf::from("/r"),
            repo_name: "r".into(),
            branch: "b".into(),
            base_branch: None,
            worktree_path: std::path::PathBuf::from("/r/wt"),
            slug: "b".into(),
            session_name: "z-r-b".into(),
            status,
            agent_name: "claude".into(),
            last_pane_hash: None,
            last_attached_count: None,
            quiet_captures: 0,
            seen_activity_since_seed: false,
            was_spinner_visible: false,
                consecutive_emits: 0,
        }
    }

    #[test]
    fn shows_spinner_true_for_creating() {
        let a = make_agent_with_status(AgentStatus::Creating);
        assert!(a.shows_spinner());
    }

    #[test]
    fn shows_spinner_false_for_stopped() {
        let a = make_agent_with_status(AgentStatus::Stopped);
        assert!(!a.shows_spinner());
    }

    #[test]
    fn shows_spinner_false_for_error() {
        let a = make_agent_with_status(AgentStatus::Error("boom".into()));
        assert!(!a.shows_spinner());
    }

    #[test]
    fn shows_spinner_follows_was_spinner_visible_when_hash_cleared() {
        // Hash cleared (e.g. by on_session_detached) — preserve last visible
        // state so an active agent's spinner doesn't drop just because we
        // briefly lost observation.
        let mut a = make_agent_with_status(AgentStatus::Running);
        a.last_pane_hash = None;
        a.quiet_captures = 999; // not consulted in the (_, None) branch

        a.was_spinner_visible = true;
        assert!(a.shows_spinner(),
            "post-detach active agent must keep its spinner");

        a.was_spinner_visible = false;
        assert!(!a.shows_spinner(),
            "post-detach idle agent must stay idle");
    }

    #[test]
    fn shows_spinner_false_for_freshly_seeded_idle_agent() {
        // Newly-discovered agent: hash is seeded but no real activity has
        // been observed yet. Without this, every agent shows a spinner
        // for ~3.5s after z starts up — even the long-idle ones.
        let mut a = make_agent_with_status(AgentStatus::Running);
        a.last_pane_hash = Some(0x1);
        a.seen_activity_since_seed = false;
        a.was_spinner_visible = false;
        a.quiet_captures = 0;
        assert!(!a.shows_spinner());
    }

    #[test]
    fn shows_spinner_true_for_active_with_quiet_below_threshold() {
        let mut a = make_agent_with_status(AgentStatus::Running);
        a.last_pane_hash = Some(0x1);
        a.seen_activity_since_seed = true;
        a.quiet_captures = QUIET_THRESHOLD - 1;
        assert!(a.shows_spinner());
    }

    #[test]
    fn shows_spinner_false_for_active_with_quiet_at_threshold() {
        let mut a = make_agent_with_status(AgentStatus::Running);
        a.last_pane_hash = Some(0x1);
        a.seen_activity_since_seed = true;
        a.quiet_captures = QUIET_THRESHOLD;
        assert!(!a.shows_spinner());
    }

    #[test]
    fn shows_spinner_drops_post_reseed_after_threshold_quiet() {
        // After a reseed (attach/detach), seen_activity is false. An active
        // agent's spinner is preserved across the reseed via the
        // was_spinner_visible fallback — but only as long as the hash
        // stays unchanged for fewer than QUIET_THRESHOLD captures. If the
        // agent silently went idle in the gap, the spinner must eventually
        // drop without firing a notification (the notification edge gates
        // on seen_activity_since_seed, which is false here).
        let mut a = make_agent_with_status(AgentStatus::Running);
        a.last_pane_hash = Some(0x1);
        a.seen_activity_since_seed = false;
        a.was_spinner_visible = true;
        a.quiet_captures = QUIET_THRESHOLD;
        assert!(!a.shows_spinner());
    }

    #[test]
    fn skip_main_worktree() {
        let repo = PathBuf::from("/home/user/src/myapp");
        let entries = vec![(
            Worktree {
                path: PathBuf::from("/home/user/src/myapp"),
                head: "abc".into(),
                branch: Some("main".into()),
                is_main: true,
            },
            "claude".to_string(),
        )];
        let agents = discover_agents(&repo, &entries, &[]);
        assert!(agents.is_empty());
    }
}
