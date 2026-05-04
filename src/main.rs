mod config;
mod agent;
mod app;
mod notifications;
#[allow(dead_code)]
mod scm;
mod style;
mod ui;

use std::time::Duration;

use crossterm::{
    event::{DisableFocusChange, EnableFocusChange, EventStream, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    execute,
};
use futures::StreamExt;
use ratatui::prelude::*;
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use app::{Action, App, Command};

/// Events produced by the dedicated event-reading task.
enum Event {
    Key(crossterm::event::KeyEvent),
    Resize,
    Focus(bool),
    Tick,
}

/// Manages the event-producing background task.
struct EventHandle {
    rx: mpsc::UnboundedReceiver<Event>,
    cancel: CancellationToken,
}

impl EventHandle {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();

        tokio::spawn(async move {
            let mut reader = EventStream::new();
            let mut tick = tokio::time::interval(Duration::from_millis(100));
            tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = cancel_clone.cancelled() => break,
                    maybe_event = reader.next() => {
                        if let Some(Ok(evt)) = maybe_event {
                            let mapped = match evt {
                                crossterm::event::Event::Key(key) => Some(Event::Key(key)),
                                crossterm::event::Event::Resize(_, _) => Some(Event::Resize),
                                crossterm::event::Event::FocusGained => Some(Event::Focus(true)),
                                crossterm::event::Event::FocusLost => Some(Event::Focus(false)),
                                _ => None,
                            };
                            if let Some(e) = mapped
                                && tx.send(e).is_err() { break; }
                        }
                    }
                    _ = tick.tick() => {
                        if tx.send(Event::Tick).is_err() { break; }
                    }
                }
            }
        });

        Self { rx, cancel }
    }

    /// Stop the event task and drain buffered events.
    fn stop(&mut self) {
        self.cancel.cancel();
        while self.rx.try_recv().is_ok() {}
    }

    /// Restart with a fresh event task (after terminal resume).
    fn restart(&mut self) {
        *self = Self::new();
    }
}

enum Cli {
    Tui,
    Version,
    Destroy { yes: bool, preserve_tmux: bool },
}

fn parse_args(args: &[String]) -> Result<Cli, String> {
    match args.first().map(String::as_str) {
        None => Ok(Cli::Tui),
        Some("-v") | Some("--version") => {
            if let Some(extra) = args.get(1) {
                return Err(format!("unexpected argument: {extra}"));
            }
            Ok(Cli::Version)
        }
        Some("destroy") => {
            let mut yes = false;
            let mut preserve_tmux = false;
            for a in &args[1..] {
                match a.as_str() {
                    "-y" | "--yes" => yes = true,
                    "--preserve-tmux" | "--leave-tmux" => preserve_tmux = true,
                    other => return Err(format!("unexpected argument: {other}")),
                }
            }
            Ok(Cli::Destroy { yes, preserve_tmux })
        }
        Some(other) => Err(format!("unknown command: {other}")),
    }
}

async fn destroy_all(
    config: &config::Config,
    yes: bool,
    preserve_tmux: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let repos = config.resolved_repos();
    let agents = agent::discover_all(&repos).await;

    if agents.is_empty() {
        println!("No agents to destroy.");
        return Ok(());
    }

    let max_session = agents.iter().map(|a| a.session_name.len()).max().unwrap_or(0);
    let n = agents.len();
    println!("About to destroy {n} agent{}:", if n == 1 { "" } else { "s" });
    if preserve_tmux {
        println!("tmux sessions will be preserved.");
    }
    for a in &agents {
        println!(
            "  {:<width$}  ({}/{})",
            a.session_name,
            a.repo_name,
            a.branch,
            width = max_session
        );
    }

    if !yes {
        use std::io::Write;
        print!("Continue? [y/N] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !matches!(input.trim(), "y" | "Y" | "yes" | "YES") {
            println!("Aborted.");
            return Ok(());
        }
    }

    let mut ok = 0usize;
    let mut failed = 0usize;
    for a in &agents {
        use std::io::Write;
        print!("Destroying {} ... ", a.session_name);
        std::io::stdout().flush()?;

        let mut errors: Vec<String> = Vec::new();
        if a.status.has_session() && !preserve_tmux {
            if let Err(e) = agent::kill_session(&a.session_name).await {
                errors.push(format!("kill_session: {e}"));
            }
        }
        if let Err(e) = agent::remove_worktree(&a.repo_path, &a.worktree_path).await {
            errors.push(format!("remove_worktree: {e}"));
        }
        if let Err(e) = agent::delete_branch(&a.repo_path, &a.branch).await {
            errors.push(format!("delete_branch: {e}"));
        }

        if errors.is_empty() {
            println!("done");
            ok += 1;
        } else {
            println!("FAILED");
            for err in errors {
                println!("  - {err}");
            }
            failed += 1;
        }
    }

    println!();
    println!("Destroyed {ok}, failed {failed}.");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&raw_args) {
        Ok(Cli::Version) => {
            println!("z {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Ok(Cli::Destroy { yes, preserve_tmux }) => {
            let config = config::Config::load()?;
            return destroy_all(&config, yes, preserve_tmux).await;
        }
        Ok(Cli::Tui) => {}
        Err(msg) => {
            eprintln!("z: {msg}");
            std::process::exit(2);
        }
    }

    let config = config::Config::load()?;
    agent::enable_tmux_focus_events();
    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<Action>();

    let mut app = App::new(config);

    // Initial async discovery
    for cmd in app.update(Action::RefreshAgents) {
        execute(cmd, &action_tx);
    }

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableFocusChange)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut events = EventHandle::new();

    // Paint the first frame immediately so startup feels instant.
    terminal.draw(|f| ui::draw(f, &app))?;
    app.dirty = false;

    loop {
        tokio::select! {
            Some(event) = events.rx.recv() => {
                match event {
                    Event::Key(key) => {
                        if key.kind == KeyEventKind::Press {
                            app.status_message = None;
                            if let Some(action) = app.handle_key(key) {
                                for cmd in app.update(action) {
                                    dispatch(cmd, &mut app, &mut terminal, &mut events, &action_tx).await?;
                                }
                            }
                        }
                    }
                    Event::Resize => {
                        app.dirty = true;
                    }
                    Event::Focus(focused) => {
                        for cmd in app.update(Action::TerminalFocus(focused)) {
                            execute(cmd, &action_tx);
                        }
                    }
                    Event::Tick => {
                        for cmd in app.update(Action::Tick) {
                            execute(cmd, &action_tx);
                        }
                    }
                }
            }
            Some(action) = action_rx.recv() => {
                for cmd in app.update(action) {
                    dispatch(cmd, &mut app, &mut terminal, &mut events, &action_tx).await?;
                }
            }
        }

        if app.should_quit {
            break;
        }

        // Render synchronously after each event/action. This removes the
        // ~33ms latency floor that a separate render ticker imposed on every
        // keystroke, while keeping ratatui's back-buffer diff to minimize
        // actual terminal traffic.
        if app.dirty {
            terminal.draw(|f| ui::draw(f, &app))?;
            app.dirty = false;
        }
    }

    // Clean shutdown
    events.stop();
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), DisableFocusChange, LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

/// Route a command. Attach is the only command that runs synchronously in
/// the main loop (it has to suspend the TUI and hand the terminal to tmux);
/// everything else is fire-and-forget via `execute`.
async fn dispatch(
    cmd: Command,
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    events: &mut EventHandle,
    action_tx: &mpsc::UnboundedSender<Action>,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        Command::Attach(agent) => {
            suspend_and_attach(app, &agent, terminal, events, action_tx).await?;
        }
        other => execute(other, action_tx),
    }
    Ok(())
}

fn execute(cmd: Command, tx: &mpsc::UnboundedSender<Action>) {
    match cmd {
        Command::Discover(repos) => {
            let tx = tx.clone();
            tokio::spawn(async move {
                let agents = agent::discover_all(&repos).await;
                let _ = tx.send(Action::AgentsRefreshed(agents));
            });
        }
        Command::LoadBranches(repo) => {
            let tx = tx.clone();
            tokio::spawn(async move {
                // Fetch first so remote tracking refs are current.
                // Swallow errors — offline/VPN-down shouldn't block the wizard.
                let _ = agent::fetch_origin(&repo).await;
                let branches = agent::list_branches(&repo).await.unwrap_or_default();
                let _ = tx.send(Action::BranchesLoaded { branches });
            });
        }
        Command::CaptureActivity { session_name } => {
            let tx = tx.clone();
            tokio::spawn(async move {
                let (content_opt, attached_opt) = tokio::join!(
                    agent::capture_pane(&session_name),
                    agent::session_attached_count(&session_name),
                );
                if let (Some(content), Some(attached_count)) = (content_opt, attached_opt) {
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut h = DefaultHasher::new();
                    content.hash(&mut h);
                    let content_hash = h.finish();
                    let _ = tx.send(Action::ActivityCaptured {
                        session_name,
                        content: Some(content),
                        content_hash,
                        attached_count,
                    });
                }
            });
        }
        Command::CreateAgent {
            repo,
            branch,
            new_branch,
            base_branch,
            session_name,
            agent_name,
            fresh_cmd,
        } => {
            let tx = tx.clone();
            tokio::spawn(async move {
                match agent::create_worktree(&repo, &branch, new_branch, base_branch.as_deref(), &agent_name).await {
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
        Command::KillSession(name) => {
            let tx = tx.clone();
            tokio::spawn(async move {
                let _ = agent::kill_session(&name).await;
                let _ = tx.send(Action::RefreshAgents);
            });
        }
        Command::DeleteAgent {
            session_name,
            kill_session,
            repo_path,
            worktree_path,
            branch,
        } => {
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut errors: Vec<String> = Vec::new();
                if kill_session && let Err(e) = agent::kill_session(&session_name).await {
                    errors.push(format!("kill tmux: {e}"));
                }
                if let Err(e) = agent::remove_worktree(&repo_path, &worktree_path).await {
                    errors.push(format!("worktree: {e}"));
                }
                if let Err(e) = agent::delete_branch(&repo_path, &branch).await {
                    errors.push(format!("branch: {e}"));
                }
                if !errors.is_empty() {
                    let _ = tx.send(Action::DeleteFailed { branch: branch.clone(), error: errors.join("; ") });
                }
                let _ = tx.send(Action::RefreshAgents);
            });
        }
        Command::PrepareAttach { agent, resume_cmd } => {
            let tx = tx.clone();
            tokio::spawn(async move {
                if !agent.status.has_session() {
                    if let Err(e) = agent::create_session(
                        &agent.session_name,
                        &agent.worktree_path,
                        Some(&resume_cmd),
                    ).await {
                        let _ = tx.send(Action::AgentFailed {
                            session: agent.session_name.clone(),
                            error: e,
                        });
                        return;
                    }
                }
                let _ = tx.send(Action::AttachReady(agent));
            });
        }
        Command::Attach(_) => unreachable!("Attach handled by dispatch"),
        Command::RefreshMergeRequests(repos) => {
            let _ = repos.len();
        }
    }
}

async fn suspend_and_attach(
    app: &mut App,
    agent_to_attach: &agent::Agent,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    events: &mut EventHandle,
    action_tx: &mpsc::UnboundedSender<Action>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Sessions are pre-created via Command::PrepareAttach before this runs,
    // so the main loop stays responsive and we never await tmux setup here.
    events.stop();

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), DisableFocusChange, LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    let _ = agent::attach(&agent_to_attach.session_name);

    // Erase tmux's "[detached ...]" line
    execute!(
        terminal.backend_mut(),
        crossterm::cursor::MoveUp(1),
        crossterm::terminal::Clear(crossterm::terminal::ClearType::FromCursorDown)
    )?;

    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen, EnableFocusChange)?;
    terminal.hide_cursor()?;
    terminal.clear()?;

    events.restart();

    // Detach reflows the pane (tmux resizes back from the client's geometry),
    // so the first post-detach capture-pane hash differs from the pre-attach
    // one even when the agent emitted nothing. The handler clears
    // `last_pane_hash` to force a fresh seed; observation-based hysteresis
    // means we don't need to do anything else to suppress spurious "done"
    // edges across the attach gap.
    app.on_session_detached(&agent_to_attach.session_name);

    for cmd in app.update(Action::RefreshAgents) {
        execute(cmd, action_tx);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_no_args_is_tui() {
        assert!(matches!(parse_args(&args(&[])), Ok(Cli::Tui)));
    }

    #[test]
    fn parse_version_flags() {
        assert!(matches!(parse_args(&args(&["-v"])), Ok(Cli::Version)));
        assert!(matches!(parse_args(&args(&["--version"])), Ok(Cli::Version)));
    }

    #[test]
    fn parse_version_rejects_extra() {
        assert!(parse_args(&args(&["-v", "foo"])).is_err());
    }

    #[test]
    fn parse_destroy_without_yes() {
        assert!(matches!(
            parse_args(&args(&["destroy"])),
            Ok(Cli::Destroy { yes: false, preserve_tmux: false })
        ));
    }

    #[test]
    fn parse_destroy_with_yes() {
        assert!(matches!(
            parse_args(&args(&["destroy", "-y"])),
            Ok(Cli::Destroy { yes: true, preserve_tmux: false })
        ));
        assert!(matches!(
            parse_args(&args(&["destroy", "--yes"])),
            Ok(Cli::Destroy { yes: true, preserve_tmux: false })
        ));
    }

    #[test]
    fn parse_destroy_with_preserve_tmux() {
        assert!(matches!(
            parse_args(&args(&["destroy", "--preserve-tmux"])),
            Ok(Cli::Destroy { yes: false, preserve_tmux: true })
        ));
        assert!(matches!(
            parse_args(&args(&["destroy", "--leave-tmux", "--yes"])),
            Ok(Cli::Destroy { yes: true, preserve_tmux: true })
        ));
    }

    #[test]
    fn parse_destroy_rejects_unknown_flag() {
        assert!(parse_args(&args(&["destroy", "--bogus"])).is_err());
    }

    #[test]
    fn parse_unknown_command() {
        assert!(parse_args(&args(&["whoops"])).is_err());
    }
}
