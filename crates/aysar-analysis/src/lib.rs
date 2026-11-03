//! Analysis engines.
//!
//! Aysar does not rewrite disassemblers and decompilers. It drives existing
//! engines through one trait, [`Engine`], and records what they say as
//! evidence. The first adapter is for `craft-anything-cli`
//! (<https://github.com/storytold/craft-anything>), whose 300-odd tools cover
//! native, .NET, Java, Android, Apple, Electron, firmware and game files.
//!
//! Engines are only consulted in **Deep** mode. In Clean room, [`Engine::run`]
//! is never called on a target's internals.

use aysar_core::Mode;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

/// What an engine tool returned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub engine: String,
    pub tool: String,
    pub ok: bool,
    /// Parsed JSON if the engine spoke JSON, else `null`.
    pub json: serde_json::Value,
    /// Raw stdout, truncated.
    pub stdout: String,
    pub stderr: String,
}

/// An external analysis engine.
pub trait Engine: Send + Sync {
    fn name(&self) -> &str;
    /// Is the engine installed and runnable?
    fn available(&self) -> bool;
    /// A version string, if it can be read.
    fn version(&self) -> Option<String>;
    /// Run one tool with arguments. `mode` is enforced here: Clean room runs
    /// are refused for tools that look inside a target.
    fn run(&self, tool: &str, args: &[String], mode: Mode) -> anyhow::Result<ToolOutput>;
}

/// Adapter for `craft-anything-cli`.
#[derive(Debug, Clone)]
pub struct CraftAnything {
    pub binary: String,
}

impl Default for CraftAnything {
    fn default() -> Self {
        CraftAnything {
            binary: std::env::var("AYSAR_CRAFT_ANYTHING_BIN").unwrap_or_else(|_| "craft-anything-cli".into()),
        }
    }
}

impl Engine for CraftAnything {
    fn name(&self) -> &str {
        "craft-anything"
    }

    fn available(&self) -> bool {
        Command::new(&self.binary).arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
    }

    fn version(&self) -> Option<String> {
        Command::new(&self.binary)
            .arg("--version")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    }

    fn run(&self, tool: &str, args: &[String], mode: Mode) -> anyhow::Result<ToolOutput> {
        if !mode.is_deep() {
            anyhow::bail!(
                "Clean room: `{tool}` would look inside the target. Switch to Deep mode (`--deep`) to use analysis engines."
            );
        }
        let mut cmd = Command::new(&self.binary);
        cmd.arg(tool).arg("--deep").args(args);
        let out = cmd.output()?;
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        let json = serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);
        Ok(ToolOutput {
            engine: self.name().into(),
            tool: tool.into(),
            ok: out.status.success(),
            json,
            stdout: truncate(&stdout, 20_000),
            stderr: truncate(&stderr, 4_000),
        })
    }
}

/// A do-nothing engine, used when nothing is installed so the pipeline still runs.
#[derive(Debug, Default, Clone)]
pub struct NullEngine;

impl Engine for NullEngine {
    fn name(&self) -> &str {
        "none"
    }
    fn available(&self) -> bool {
        true
    }
    fn version(&self) -> Option<String> {
        None
    }
    fn run(&self, tool: &str, _args: &[String], _mode: Mode) -> anyhow::Result<ToolOutput> {
        Ok(ToolOutput {
            engine: "none".into(),
            tool: tool.into(),
            ok: false,
            json: serde_json::Value::Null,
            stdout: String::new(),
            stderr: "no analysis engine installed; install craft-anything-cli for app/game/binary analysis".into(),
        })
    }
}

/// Pick the best engine available on this machine.
pub fn detect() -> Box<dyn Engine> {
    let ca = CraftAnything::default();
    if ca.available() {
        Box::new(ca)
    } else {
        Box::new(NullEngine)
    }
}

/// A quick identification of a local file, without an engine: by extension
/// and magic bytes. Enough to route it, not to analyse it.
pub fn identify(path: &Path) -> anyhow::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut head = [0u8; 16];
    let n = f.read(&mut head)?;
    let head = &head[..n];
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let by_magic = if head.starts_with(b"MZ") {
        Some("Windows PE executable")
    } else if head.starts_with(b"\x7fELF") {
        Some("ELF executable (Linux/BSD/Android)")
    } else if head.starts_with(&[0xCF, 0xFA, 0xED, 0xFE]) || head.starts_with(&[0xCA, 0xFE, 0xBA, 0xBE]) {
        Some("Mach-O executable (macOS/iOS)")
    } else if head.starts_with(b"PK\x03\x04") {
        Some(match ext.as_str() {
            "apk" => "Android package (APK)",
            "jar" => "Java archive (JAR)",
            "ipa" => "iOS package (IPA)",
            "docx" | "xlsx" | "pptx" => "Office document",
            _ => "ZIP archive",
        })
    } else if head.starts_with(b"\0asm") {
        Some("WebAssembly module")
    } else if head.starts_with(b"<!") || head.starts_with(b"<html") || head.starts_with(b"<HTML") {
        Some("HTML document")
    } else if head.starts_with(b"%PDF") {
        Some("PDF document")
    } else if head.starts_with(b"SQLite format 3") {
        Some("SQLite database")
    } else {
        None
    };
    Ok(by_magic.map(|s| s.to_string()).unwrap_or_else(|| {
        if ext.is_empty() {
            "unknown".into()
        } else {
            format!("file with extension .{ext}")
        }
    }))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut cut = max;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…[truncated]", &s[..cut])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_room_refuses_engine() {
        let ca = CraftAnything { binary: "definitely-not-installed-xyz".into() };
        let err = ca.run("functions", &[], Mode::CleanRoom).unwrap_err();
        assert!(err.to_string().contains("Clean room"));
    }

    #[test]
    fn identify_html() {
        let p = std::env::temp_dir().join(format!("aysar-id-{}.html", std::process::id()));
        std::fs::write(&p, "<!doctype html><html></html>").unwrap();
        assert_eq!(identify(&p).unwrap(), "HTML document");
        let _ = std::fs::remove_file(p);
    }
}
