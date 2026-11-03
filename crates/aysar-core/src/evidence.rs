//! The evidence ledger: every claim Aysar makes is recorded with where it
//! came from, how confident we are, and which other claims it supports.
//!
//! Storage is an append-only JSON Lines file (`evidence/ledger.jsonl`),
//! which is diff-friendly, grep-friendly and needs no native database.
//! A graph database can replace it later without changing this API.

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceKind {
    /// Something directly observed (a heading, a colour, a link).
    Observation,
    /// Something inferred from observations.
    Inference,
    /// A result of comparing the build to the original.
    Check,
    /// A decision the user made.
    Decision,
    /// A step the agent or the pipeline took.
    Action,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub kind: EvidenceKind,
    /// The claim in plain words.
    pub claim: String,
    /// Where it came from: a URL, a path, a tool name, a check id.
    pub source: String,
    /// 0.0 – 1.0.
    pub confidence: f32,
    /// Ids of other evidence this one supports.
    pub supports: Vec<String>,
    pub recorded_at: String,
}

/// An append-only ledger on disk.
#[derive(Debug, Clone)]
pub struct Ledger {
    path: PathBuf,
}

impl Ledger {
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Ledger> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if !path.exists() {
            fs::write(&path, "")?;
        }
        Ok(Ledger { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Record a claim and return its id.
    pub fn record(
        &self,
        kind: EvidenceKind,
        claim: impl Into<String>,
        source: impl Into<String>,
        confidence: f32,
        supports: Vec<String>,
    ) -> anyhow::Result<String> {
        let n = self.count()?;
        let id = format!("ev-{:05}", n + 1);
        let ev = Evidence {
            id: id.clone(),
            kind,
            claim: claim.into(),
            source: source.into(),
            confidence: confidence.clamp(0.0, 1.0),
            supports,
            recorded_at: crate::now_rfc3339(),
        };
        let mut f = OpenOptions::new().append(true).open(&self.path)?;
        writeln!(f, "{}", serde_json::to_string(&ev)?)?;
        Ok(id)
    }

    pub fn count(&self) -> anyhow::Result<usize> {
        let f = fs::File::open(&self.path)?;
        Ok(BufReader::new(f).lines().filter(|l| l.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false)).count())
    }

    pub fn all(&self) -> anyhow::Result<Vec<Evidence>> {
        let f = fs::File::open(&self.path)?;
        let mut out = Vec::new();
        for line in BufReader::new(f).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            out.push(serde_json::from_str(&line)?);
        }
        Ok(out)
    }

    pub fn by_kind(&self, kind: EvidenceKind) -> anyhow::Result<Vec<Evidence>> {
        Ok(self.all()?.into_iter().filter(|e| e.kind == kind).collect())
    }

    /// Everything that (transitively) supports the given id.
    pub fn supporters_of(&self, id: &str) -> anyhow::Result<Vec<Evidence>> {
        let all = self.all()?;
        let mut wanted = vec![id.to_string()];
        let mut found: Vec<Evidence> = Vec::new();
        let mut i = 0;
        while i < wanted.len() {
            let target = wanted[i].clone();
            for e in &all {
                if e.supports.iter().any(|s| s == &target) && !found.iter().any(|f| f.id == e.id) {
                    found.push(e.clone());
                    wanted.push(e.id.clone());
                }
            }
            i += 1;
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_query() {
        let dir = std::env::temp_dir().join(format!("aysar-ledger-{}", std::process::id()));
        let ledger = Ledger::open(dir.join("ledger.jsonl")).unwrap();
        let a = ledger.record(EvidenceKind::Observation, "h1 says Hello", "https://x", 1.0, vec![]).unwrap();
        let b = ledger.record(EvidenceKind::Inference, "page is a landing page", "study", 0.7, vec![]).unwrap();
        let _c = ledger.record(EvidenceKind::Observation, "has a form", "https://x", 1.0, vec![b.clone()]).unwrap();
        assert_eq!(a, "ev-00001");
        assert_eq!(ledger.count().unwrap(), 3);
        assert_eq!(ledger.by_kind(EvidenceKind::Observation).unwrap().len(), 2);
        assert_eq!(ledger.supporters_of(&b).unwrap().len(), 1);
        let _ = fs::remove_dir_all(dir);
    }
}
