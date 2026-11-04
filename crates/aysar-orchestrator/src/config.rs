//! User configuration: `~/.config/aysar/config.toml` (or `%APPDATA%\aysar\config.toml`),
//! overridden by environment variables, overridden by command-line flags.
//!
//! ```toml
//! projects_dir = "/home/me/Documents/Aysar"
//! agent = "claude-code"          # or codex, aider, gemini, opencode, goose, dry-run, a template
//! chrome = "/usr/bin/chromium"
//! rounds = 2
//! target_score = 85
//! max_pages = 5
//!
//! [llm]                           # for `aysar enrich`; any OpenAI-compatible server
//! url = "http://localhost:11434/v1"
//! model = "llama3.1"
//! key = ""
//! ```

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub projects_dir: Option<PathBuf>,
    pub agent: Option<String>,
    pub chrome: Option<PathBuf>,
    pub rounds: Option<usize>,
    pub target_score: Option<f32>,
    pub max_pages: Option<usize>,
    pub llm: Llm,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Llm {
    pub url: Option<String>,
    pub model: Option<String>,
    pub key: Option<String>,
}

impl Config {
    /// The file we read, whether or not it exists.
    pub fn path() -> PathBuf {
        if let Ok(p) = std::env::var("AYSAR_CONFIG") {
            return PathBuf::from(p);
        }
        let base = if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        };
        base.unwrap_or_else(|| PathBuf::from(".")).join("aysar").join("config.toml")
    }

    /// Load the file (empty config if absent), then apply environment overrides.
    pub fn load() -> Config {
        let mut c = Self::from_file(&Self::path()).unwrap_or_default();
        c.apply_env();
        c
    }

    pub fn from_file(path: &Path) -> anyhow::Result<Config> {
        let text = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    /// `AYSAR_PROJECTS_DIR`, `AYSAR_AGENT`, `AYSAR_CHROME`, `AYSAR_LLM_URL`,
    /// `AYSAR_LLM_MODEL`, `AYSAR_LLM_KEY` override the file.
    pub fn apply_env(&mut self) {
        if let Ok(v) = std::env::var("AYSAR_PROJECTS_DIR") {
            self.projects_dir = Some(PathBuf::from(v));
        }
        if let Ok(v) = std::env::var("AYSAR_AGENT") {
            self.agent = Some(v);
        }
        if let Ok(v) = std::env::var("AYSAR_CHROME") {
            self.chrome = Some(PathBuf::from(v));
        }
        if let Ok(v) = std::env::var("AYSAR_LLM_URL") {
            self.llm.url = Some(v);
        }
        if let Ok(v) = std::env::var("AYSAR_LLM_MODEL") {
            self.llm.model = Some(v);
        }
        if let Ok(v) = std::env::var("AYSAR_LLM_KEY") {
            self.llm.key = Some(v);
        }
    }

    /// Push file settings into the environment the lower crates read.
    pub fn export_env(&self) {
        if let Some(c) = &self.chrome {
            if std::env::var_os("AYSAR_CHROME").is_none() {
                std::env::set_var("AYSAR_CHROME", c);
            }
        }
        if let Some(p) = &self.projects_dir {
            if std::env::var_os("AYSAR_PROJECTS_DIR").is_none() {
                std::env::set_var("AYSAR_PROJECTS_DIR", p);
            }
        }
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("aysar-cfg-{}", std::process::id()));
        let path = dir.join("config.toml");
        let c = Config {
            projects_dir: Some(PathBuf::from("/x")),
            agent: Some("codex".into()),
            rounds: Some(3),
            llm: Llm { url: Some("http://localhost:11434/v1".into()), model: Some("llama3.1".into()), key: None },
            ..Default::default()
        };
        c.save(&path).unwrap();
        let back = Config::from_file(&path).unwrap();
        assert_eq!(back, c);
        let partial: Config = toml::from_str("agent = \"aider\"\n[llm]\nmodel = \"m\"\n").unwrap();
        assert_eq!(partial.agent.as_deref(), Some("aider"));
        assert_eq!(partial.rounds, None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
