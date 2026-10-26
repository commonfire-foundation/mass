use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub agent: String,
    pub root: PathBuf,
    #[serde(default)]
    pub resume: bool,
    #[serde(default)]
    pub optional: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub sources: Vec<Source>,
    #[serde(default)]
    pub exclusions: Vec<String>,
}
pub fn xdg(key: &str, fallback: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback)
        })
        .join("mass")
}
pub fn load(path: Option<&Path>) -> Result<Config> {
    let default = xdg("XDG_CONFIG_HOME", ".config").join("config.toml");
    let path = path.unwrap_or(&default);
    let config = if path.exists() {
        toml::from_str(&std::fs::read_to_string(path)?)?
    } else if path != default {
        bail!("Configuration not found: {}", path.display());
    } else {
        let home = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?);
        Config {
            sources: vec![
                Source {
                    id: "pi".into(),
                    agent: "pi".into(),
                    root: home.join(".pi/agent/sessions"),
                    resume: false,
                    optional: true,
                },
                Source {
                    id: "claude".into(),
                    agent: "claude".into(),
                    root: home.join(".claude/projects"),
                    resume: false,
                    optional: true,
                },
                Source {
                    id: "codex".into(),
                    agent: "codex".into(),
                    root: home.join(".codex/sessions"),
                    resume: false,
                    optional: true,
                },
                Source {
                    id: "codex-archive".into(),
                    agent: "codex".into(),
                    root: home.join(".codex/archived_sessions"),
                    resume: false,
                    optional: true,
                },
            ],
            exclusions: vec![],
        }
    };
    let mut ids = std::collections::HashSet::new();
    for source in &config.sources {
        if source.id.is_empty() || !ids.insert(&source.id) {
            bail!("Source IDs must be nonempty and unique");
        }
        if !matches!(source.agent.as_str(), "pi" | "claude" | "codex") {
            bail!("Unsupported agent: {}", source.agent);
        }
        if !source.root.is_absolute() {
            bail!("Source roots must be absolute: {}", source.root.display());
        }
    }
    Ok(config)
}
