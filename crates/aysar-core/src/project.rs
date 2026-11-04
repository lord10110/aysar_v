//! A project on disk.
//!
//! ```text
//! <root>/
//!   aysar.json        request, target, mode, status, history
//!   capture/          what was observed (capture.json, raw/ in deep mode)
//!   spec/             spec.json + spec.md
//!   app/              the generated project
//!   checks/           check-001.json, check-001.md, ...
//!   evidence/         ledger.jsonl
//!   inbox/            open questions for the user
//! ```

use crate::{Kind, Mode, Target};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const STATE_FILE: &str = "aysar.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    #[default]
    New,
    Studied,
    Specified,
    Built,
    Checked,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub at: String,
    pub stage: Stage,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectState {
    pub name: String,
    pub request: String,
    pub target: Target,
    pub kind: Kind,
    pub mode: Mode,
    pub stage: Stage,
    pub created_at: String,
    pub updated_at: String,
    /// Which coding agent built it, if any.
    pub agent: Option<String>,
    /// Latest overall similarity score, 0–100.
    pub score: Option<f32>,
    /// When several agents competed: (agent, score).
    #[serde(default)]
    pub candidates: Vec<(String, Option<f32>)>,
    pub history: Vec<HistoryEntry>,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub state: ProjectState,
}

impl Project {
    /// Create a fresh project directory.
    pub fn create(
        root: impl AsRef<Path>,
        name: &str,
        request: &str,
        target: Target,
        mode: Mode,
    ) -> anyhow::Result<Project> {
        let root = root.as_ref().to_path_buf();
        for sub in ["capture", "spec", "app", "checks", "evidence", "inbox"] {
            fs::create_dir_all(root.join(sub))?;
        }
        let now = crate::now_rfc3339();
        let kind = target.kind_hint();
        let state = ProjectState {
            name: name.to_string(),
            request: request.to_string(),
            kind,
            target,
            mode,
            stage: Stage::New,
            created_at: now.clone(),
            updated_at: now.clone(),
            agent: None,
            score: None,
            candidates: vec![],
            history: vec![HistoryEntry { at: now, stage: Stage::New, note: "project created".into() }],
        };
        let p = Project { root, state };
        p.save()?;
        Ok(p)
    }

    /// Open an existing project.
    pub fn open(root: impl AsRef<Path>) -> anyhow::Result<Project> {
        let root = root.as_ref().to_path_buf();
        let s = fs::read_to_string(root.join(STATE_FILE))?;
        let state: ProjectState = serde_json::from_str(&s)?;
        Ok(Project { root, state })
    }

    pub fn exists(root: impl AsRef<Path>) -> bool {
        root.as_ref().join(STATE_FILE).exists()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        fs::write(self.root.join(STATE_FILE), serde_json::to_string_pretty(&self.state)?)?;
        Ok(())
    }

    pub fn advance(&mut self, stage: Stage, note: impl Into<String>) -> anyhow::Result<()> {
        let now = crate::now_rfc3339();
        self.state.stage = stage;
        self.state.updated_at = now.clone();
        self.state.history.push(HistoryEntry { at: now, stage, note: note.into() });
        self.save()
    }

    pub fn capture_dir(&self) -> PathBuf {
        self.root.join("capture")
    }
    pub fn capture_file(&self) -> PathBuf {
        self.capture_dir().join("capture.json")
    }
    pub fn spec_dir(&self) -> PathBuf {
        self.root.join("spec")
    }
    pub fn spec_file(&self) -> PathBuf {
        self.spec_dir().join("spec.json")
    }
    pub fn spec_markdown(&self) -> PathBuf {
        self.spec_dir().join("spec.md")
    }
    pub fn app_dir(&self) -> PathBuf {
        self.root.join("app")
    }
    pub fn checks_dir(&self) -> PathBuf {
        self.root.join("checks")
    }
    pub fn ledger_file(&self) -> PathBuf {
        self.root.join("evidence").join("ledger.jsonl")
    }
    pub fn inbox_dir(&self) -> PathBuf {
        self.root.join("inbox")
    }

    /// Next check number, by counting existing `check-NNN.json` files.
    pub fn next_check_number(&self) -> usize {
        let mut n = 0;
        if let Ok(rd) = fs::read_dir(self.checks_dir()) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("check-") && name.ends_with(".json") {
                    n += 1;
                }
            }
        }
        n + 1
    }

    /// Leave a question for the user in `inbox/`.
    pub fn ask(&self, question: &str) -> anyhow::Result<PathBuf> {
        let n = fs::read_dir(self.inbox_dir()).map(|r| r.count()).unwrap_or(0) + 1;
        let path = self.inbox_dir().join(format!("question-{n:03}.md"));
        fs::write(
            &path,
            format!("# Question {n}\n\n{question}\n\n_Answer by editing this file, or pass `--answer` on the CLI._\n"),
        )?;
        Ok(path)
    }
}

/// Turn a request into a safe folder name.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
        if out.len() >= 48 {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "project".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug() {
        assert_eq!(slugify("Recreate this website!"), "recreate-this-website");
        assert_eq!(slugify("   "), "project");
    }

    #[test]
    fn create_open_advance() {
        let root = std::env::temp_dir().join(format!("aysar-proj-{}", std::process::id()));
        let mut p =
            Project::create(&root, "demo", "make it", Target::parse("https://example.com"), Mode::CleanRoom).unwrap();
        assert!(Project::exists(&root));
        p.advance(Stage::Studied, "studied").unwrap();
        let q = Project::open(&root).unwrap();
        assert_eq!(q.state.stage, Stage::Studied);
        assert_eq!(q.state.kind, Kind::Website);
        assert_eq!(q.next_check_number(), 1);
        let _ = fs::remove_dir_all(root);
    }
}
