use serde::Deserialize;
use std::path::PathBuf;

const BUILTIN_DEFAULT_AGENT: &str = "claude";

#[derive(Debug, Clone, PartialEq)]
pub struct AgentDef {
    pub cmd: String,
    pub resume: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(default)]
    repos: Vec<String>,
    #[serde(default)]
    default_agent: Option<String>,
    #[serde(default)]
    agents: Option<toml::Table>,
    #[serde(default)]
    notifications: Notifications,
}

#[derive(Debug, Deserialize)]
struct RawAgent {
    cmd: String,
    #[serde(default)]
    resume: Option<String>,
}

#[derive(Debug, Default)]
pub struct Config {
    pub repos: Vec<String>,
    pub notifications: Notifications,
    pub(crate) agents: Vec<(String, AgentDef)>,
    pub(crate) default_agent: String,
}

#[derive(Debug, Deserialize)]
pub struct Notifications {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_only_when_unfocused")]
    pub only_when_unfocused: bool,
}

impl Default for Notifications {
    fn default() -> Self {
        Self {
            enabled: false,
            only_when_unfocused: true,
        }
    }
}

fn default_only_when_unfocused() -> bool {
    true
}

impl Config {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_dir = dirs::home_dir()
            .ok_or("no home directory")?
            .join(".config")
            .join("z");
        let config_path = config_dir.join("config.toml");

        if !config_path.exists() {
            std::fs::create_dir_all(&config_dir)?;
            std::fs::write(&config_path, "repos = []\n")?;
        }

        let contents = std::fs::read_to_string(&config_path)?;
        Self::from_toml_str(&contents).map_err(Into::into)
    }

    pub fn from_toml_str(s: &str) -> Result<Self, String> {
        let raw: RawConfig = toml::from_str(s).map_err(|e| format!("config parse: {e}"))?;

        let (agents, default_agent) = match raw.agents {
            None => {
                let agents = builtin_agents();
                let default = match raw.default_agent {
                    None => BUILTIN_DEFAULT_AGENT.to_string(),
                    Some(name) => resolve_default(Some(&name), &agents)?,
                };
                (agents, default)
            }
            Some(table) => {
                let agents = parse_user_agents(table)?;
                let default = resolve_default(raw.default_agent.as_deref(), &agents)?;
                (agents, default)
            }
        };

        Ok(Config {
            repos: raw.repos,
            notifications: raw.notifications,
            agents,
            default_agent,
        })
    }

    pub fn resolved_repos(&self) -> Vec<PathBuf> {
        let home = dirs::home_dir().unwrap_or_default();
        self.repos
            .iter()
            .map(|r| {
                if let Some(stripped) = r.strip_prefix("~/") {
                    home.join(stripped)
                } else {
                    PathBuf::from(r)
                }
            })
            .collect()
    }

    pub fn get_agent(&self, name: &str) -> Option<&AgentDef> {
        self.agents
            .iter()
            .find_map(|(n, def)| (n == name).then_some(def))
    }

    pub fn default_agent_name(&self) -> &str {
        &self.default_agent
    }

    pub fn fresh(&self, name: &str, prompt: Option<&str>) -> Option<String> {
        let def = self.get_agent(name)?;
        Some(match prompt {
            Some(p) => {
                let escaped = p.replace('\'', "'\\''");
                format!("{} '{}'", def.cmd, escaped)
            }
            None => def.cmd.clone(),
        })
    }

    pub fn resume(&self, name: &str) -> Option<String> {
        let def = self.get_agent(name)?;
        Some(def.resume.clone().unwrap_or_else(|| def.cmd.clone()))
    }

    pub fn cycle_next(&self, current: &str) -> &str {
        match agent_index(&self.agents, current) {
            Some(i) => &self.agents[(i + 1) % self.agents.len()].0,
            None => &self.default_agent,
        }
    }

    pub fn cycle_prev(&self, current: &str) -> &str {
        match agent_index(&self.agents, current) {
            Some(i) => {
                let len = self.agents.len();
                &self.agents[(i + len - 1) % len].0
            }
            None => &self.default_agent,
        }
    }
}

fn parse_user_agents(table: toml::Table) -> Result<Vec<(String, AgentDef)>, String> {
    if table.is_empty() {
        return Err("[agents] table is empty".to_string());
    }
    let mut out = Vec::with_capacity(table.len());
    for (name, value) in table {
        let raw: RawAgent = value
            .try_into()
            .map_err(|e| format!("agent '{name}': {e}"))?;
        if raw.cmd.trim().is_empty() {
            return Err(format!("agent '{name}': cmd is empty"));
        }
        out.push((
            name,
            AgentDef {
                cmd: raw.cmd,
                resume: raw.resume,
            },
        ));
    }
    Ok(out)
}

fn resolve_default(
    explicit: Option<&str>,
    agents: &[(String, AgentDef)],
) -> Result<String, String> {
    match explicit {
        Some(name) => {
            if agent_index(agents, name).is_none() {
                return Err(format!("default_agent '{name}' is not in [agents]"));
            }
            Ok(name.to_string())
        }
        None => match agents.len() {
            1 => Ok(agents[0].0.clone()),
            _ => Err("default_agent must be set when [agents] has more than one entry".to_string()),
        },
    }
}

fn agent_index(agents: &[(String, AgentDef)], name: &str) -> Option<usize> {
    agents.iter().position(|(n, _)| n == name)
}

fn builtin_agents() -> Vec<(String, AgentDef)> {
    vec![
        (
            "claude".to_string(),
            AgentDef {
                cmd: "claude --dangerously-skip-permissions".to_string(),
                resume: Some("claude --dangerously-skip-permissions --continue".to_string()),
            },
        ),
        (
            "codex".to_string(),
            AgentDef {
                cmd: "codex --dangerously-bypass-approvals-and-sandbox".to_string(),
                resume: Some("codex resume --last".to_string()),
            },
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_repos_only_uses_builtin_agents() {
        let toml_str = r#"repos = ["~/src/myapp"]"#;
        let config: Config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(config.repos.len(), 1);
        assert_eq!(config.default_agent_name(), "claude");
        let names: Vec<&str> = config.agents.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["claude", "codex"]);
    }

    #[test]
    fn parse_user_defined_agents() {
        let toml_str = r#"
repos = []
default_agent = "work-claude"

[agents.work-claude]
cmd = "/usr/bin/claude"

[agents.codex]
cmd = "codex --dangerously-bypass-approvals-and-sandbox"
resume = "codex resume --last"
"#;
        let config: Config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(config.default_agent_name(), "work-claude");
        let names: Vec<&str> = config.agents.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["work-claude", "codex"]);
        let work = config.get_agent("work-claude").unwrap();
        assert_eq!(work.cmd, "/usr/bin/claude");
        assert!(work.resume.is_none());
        let codex = config.get_agent("codex").unwrap();
        assert_eq!(codex.resume.as_deref(), Some("codex resume --last"));
    }

    #[test]
    fn resolve_tilde_in_paths() {
        let config = Config {
            repos: vec!["~/src/myapp".to_string()],
            ..Default::default()
        };
        let resolved = config.resolved_repos();
        assert!(!resolved[0].to_str().unwrap().contains('~'));
        assert!(resolved[0].to_str().unwrap().ends_with("/src/myapp"));
    }

    #[test]
    fn notifications_default_when_section_missing() {
        let toml_str = r#"repos = []"#;
        let config = Config::from_toml_str(toml_str).unwrap();
        assert!(!config.notifications.enabled);
        assert!(config.notifications.only_when_unfocused);
    }

    #[test]
    fn notifications_partial_section_fills_defaults() {
        let toml_str = r#"
repos = []
[notifications]
enabled = true
"#;
        let config = Config::from_toml_str(toml_str).unwrap();
        assert!(config.notifications.enabled);
        assert!(config.notifications.only_when_unfocused);
    }

    #[test]
    fn empty_agents_table_is_error() {
        let toml_str = r#"
repos = []
[agents]
"#;
        let err = Config::from_toml_str(toml_str).unwrap_err();
        assert!(err.contains("[agents] table is empty"), "got: {err}");
    }

    #[test]
    fn empty_cmd_is_error() {
        let toml_str = r#"
[agents.broken]
cmd = ""
"#;
        let err = Config::from_toml_str(toml_str).unwrap_err();
        assert!(err.contains("cmd is empty"), "got: {err}");
    }

    #[test]
    fn default_agent_not_in_table_is_error() {
        let toml_str = r#"
default_agent = "ghost"

[agents.claude]
cmd = "claude"
"#;
        let err = Config::from_toml_str(toml_str).unwrap_err();
        assert!(
            err.contains("default_agent 'ghost' is not in [agents]"),
            "got: {err}"
        );
    }

    #[test]
    fn multi_agent_without_default_is_error() {
        let toml_str = r#"
[agents.a]
cmd = "a"

[agents.b]
cmd = "b"
"#;
        let err = Config::from_toml_str(toml_str).unwrap_err();
        assert!(err.contains("default_agent must be set"), "got: {err}");
    }

    #[test]
    fn single_agent_without_default_uses_that_agent() {
        let toml_str = r#"
[agents.solo]
cmd = "solo --go"
"#;
        let config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(config.default_agent_name(), "solo");
    }

    #[test]
    fn user_defined_agents_do_not_merge_with_builtins() {
        let toml_str = r#"
[agents.solo]
cmd = "solo --go"
"#;
        let config = Config::from_toml_str(toml_str).unwrap();
        let names: Vec<&str> = config.agents.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec!["solo"],
            "builtins must not appear when user defines [agents]"
        );
    }

    #[test]
    fn fresh_with_no_prompt_returns_cmd() {
        let config = Config::from_toml_str(r#"repos = []"#).unwrap();
        assert_eq!(
            config.fresh("codex", None).unwrap(),
            "codex --dangerously-bypass-approvals-and-sandbox",
        );
    }

    #[test]
    fn fresh_appends_quoted_prompt() {
        let config = Config::from_toml_str(r#"repos = []"#).unwrap();
        assert_eq!(
            config.fresh("claude", Some("fix auth")).unwrap(),
            "claude --dangerously-skip-permissions 'fix auth'",
        );
    }

    #[test]
    fn fresh_escapes_single_quotes_in_prompt() {
        let config = Config::from_toml_str(r#"repos = []"#).unwrap();
        assert_eq!(
            config.fresh("codex", Some("it's")).unwrap(),
            "codex --dangerously-bypass-approvals-and-sandbox 'it'\\''s'",
        );
    }

    #[test]
    fn fresh_returns_none_for_unknown_agent() {
        let config = Config::from_toml_str(r#"repos = []"#).unwrap();
        assert!(config.fresh("ghost", None).is_none());
    }

    #[test]
    fn resume_uses_explicit_field() {
        let config = Config::from_toml_str(r#"repos = []"#).unwrap();
        assert_eq!(
            config.resume("claude").unwrap(),
            "claude --dangerously-skip-permissions --continue",
        );
    }

    #[test]
    fn resume_falls_back_to_cmd_when_omitted() {
        let toml_str = r#"
[agents.work-claude]
cmd = "/usr/bin/claude"
"#;
        let config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(config.resume("work-claude").unwrap(), "/usr/bin/claude");
    }

    #[test]
    fn resume_returns_none_for_unknown_agent() {
        let config = Config::from_toml_str(r#"repos = []"#).unwrap();
        assert!(config.resume("ghost").is_none());
    }

    #[test]
    fn cycle_next_wraps_in_declaration_order() {
        let toml_str = r#"
default_agent = "a"

[agents.a]
cmd = "a"

[agents.b]
cmd = "b"

[agents.c]
cmd = "c"
"#;
        let config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(config.cycle_next("a"), "b");
        assert_eq!(config.cycle_next("b"), "c");
        assert_eq!(config.cycle_next("c"), "a");
    }

    #[test]
    fn cycle_next_unknown_agent_returns_default() {
        let config = Config::from_toml_str(r#"repos = []"#).unwrap();
        assert_eq!(config.cycle_next("ghost"), config.default_agent_name());
    }

    #[test]
    fn cycle_prev_wraps_backwards() {
        let toml_str = r#"
default_agent = "a"

[agents.a]
cmd = "a"

[agents.b]
cmd = "b"

[agents.c]
cmd = "c"
"#;
        let config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(config.cycle_prev("a"), "c");
        assert_eq!(config.cycle_prev("b"), "a");
        assert_eq!(config.cycle_prev("c"), "b");
    }
}
