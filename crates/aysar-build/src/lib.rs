//! The agent gateway.
//!
//! Aysar is agent-agnostic. Every coding agent is reached through one trait,
//! [`Agent`]. Built in:
//!
//! - [`ClaudeCode`]: runs your own `claude` program non-interactively.
//! - [`Codex`]: runs `codex exec`.
//! - [`Script`]: runs any command template with `{prompt}` and `{prompt_file}`
//!   placeholders, so a new agent is one line of configuration away.
//!
//! The agent is always run **inside the project's `app/` folder** and never
//! sees the original target's files: it works from the spec and the prompt.

pub mod prompt;

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildOutput {
    pub agent: String,
    pub ok: bool,
    pub seconds: f64,
    pub stdout_tail: String,
    pub stderr_tail: String,
    /// Files present in the app folder after the run.
    pub files: Vec<String>,
}

/// A coding agent.
pub trait Agent: Send + Sync {
    fn name(&self) -> &str;
    fn available(&self) -> bool;
    /// Run the agent in `workdir` with `prompt`.
    fn run(&self, workdir: &Path, prompt: &str) -> anyhow::Result<BuildOutput>;
}

/// Claude Code. Uses the `claude` binary already signed in on this machine.
#[derive(Debug, Clone)]
pub struct ClaudeCode {
    pub binary: String,
    pub extra_args: Vec<String>,
}

impl Default for ClaudeCode {
    fn default() -> Self {
        ClaudeCode {
            binary: std::env::var("AYSAR_CLAUDE_BIN").unwrap_or_else(|_| "claude".into()),
            extra_args: std::env::var("AYSAR_CLAUDE_ARGS")
                .map(|s| s.split_whitespace().map(|x| x.to_string()).collect())
                .unwrap_or_else(|_| vec!["--permission-mode".into(), "acceptEdits".into()]),
        }
    }
}

impl Agent for ClaudeCode {
    fn name(&self) -> &str {
        "claude-code"
    }
    fn available(&self) -> bool {
        which(&self.binary)
    }
    fn run(&self, workdir: &Path, prompt: &str) -> anyhow::Result<BuildOutput> {
        let mut cmd = command_for(&self.binary);
        cmd.current_dir(workdir);
        if cfg!(windows) && prompt.len() > 24_000 {
            // Windows command lines are capped at 32 K: hand the prompt over on stdin.
            cmd.arg("-p").arg("Carry out the build instructions that follow on standard input.").args(&self.extra_args);
            return run_command_with_stdin(self.name(), cmd, workdir, prompt);
        }
        cmd.arg("-p").arg(prompt).args(&self.extra_args);
        run_command(self.name(), cmd, workdir)
    }
}

fn run_command_with_stdin(agent: &str, mut cmd: Command, workdir: &Path, input: &str) -> anyhow::Result<BuildOutput> {
    use std::io::Write;
    fs::create_dir_all(workdir)?;
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let start = Instant::now();
    let mut child = cmd.spawn().map_err(|e| anyhow::anyhow!("could not start {agent}: {e}"))?;
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(input.as_bytes());
    }
    let out = child.wait_with_output()?;
    Ok(BuildOutput {
        agent: agent.into(),
        ok: out.status.success(),
        seconds: start.elapsed().as_secs_f64(),
        stdout_tail: tail(&String::from_utf8_lossy(&out.stdout), 4000),
        stderr_tail: tail(&String::from_utf8_lossy(&out.stderr), 2000),
        files: list_files(workdir),
    })
}

/// OpenAI Codex CLI.
#[derive(Debug, Clone)]
pub struct Codex {
    pub binary: String,
    pub extra_args: Vec<String>,
}

impl Default for Codex {
    fn default() -> Self {
        Codex {
            binary: std::env::var("AYSAR_CODEX_BIN").unwrap_or_else(|_| "codex".into()),
            extra_args: std::env::var("AYSAR_CODEX_ARGS")
                .map(|s| s.split_whitespace().map(|x| x.to_string()).collect())
                .unwrap_or_else(|_| vec!["--full-auto".into()]),
        }
    }
}

impl Agent for Codex {
    fn name(&self) -> &str {
        "codex"
    }
    fn available(&self) -> bool {
        which(&self.binary)
    }
    fn run(&self, workdir: &Path, prompt: &str) -> anyhow::Result<BuildOutput> {
        let mut cmd = command_for(&self.binary);
        cmd.current_dir(workdir).arg("exec").args(&self.extra_args).arg(prompt);
        run_command(self.name(), cmd, workdir)
    }
}

/// Any command. `{prompt}` is replaced with the prompt text, `{prompt_file}`
/// with a path to a file containing it. Example:
/// `aider --message-file {prompt_file} --yes`.
#[derive(Debug, Clone)]
pub struct Script {
    pub label: String,
    pub template: String,
}

impl Agent for Script {
    fn name(&self) -> &str {
        &self.label
    }
    fn available(&self) -> bool {
        self.template.split_whitespace().next().map(which).unwrap_or(false)
    }
    fn run(&self, workdir: &Path, prompt: &str) -> anyhow::Result<BuildOutput> {
        fs::create_dir_all(workdir)?;
        let prompt_file = workdir.join(".aysar-prompt.md");
        fs::write(&prompt_file, prompt)?;
        let parts: Vec<String> = shell_split(&self.template)
            .into_iter()
            .map(|p| p.replace("{prompt_file}", &prompt_file.display().to_string()).replace("{prompt}", prompt))
            .collect();
        let (bin, rest) = parts.split_first().ok_or_else(|| anyhow::anyhow!("empty agent template"))?;
        let mut cmd = command_for(bin);
        cmd.current_dir(workdir).args(rest);
        let out = run_command(&self.label, cmd, workdir);
        let _ = fs::remove_file(&prompt_file);
        out
    }
}

/// A dry-run agent that writes the prompt to `BUILD-PROMPT.md` and does
/// nothing else. Lets the whole pipeline run with no AI installed.
#[derive(Debug, Default, Clone)]
pub struct DryRun;

impl Agent for DryRun {
    fn name(&self) -> &str {
        "dry-run"
    }
    fn available(&self) -> bool {
        true
    }
    fn run(&self, workdir: &Path, prompt: &str) -> anyhow::Result<BuildOutput> {
        fs::create_dir_all(workdir)?;
        fs::write(workdir.join("BUILD-PROMPT.md"), prompt)?;
        Ok(BuildOutput {
            agent: "dry-run".into(),
            ok: true,
            seconds: 0.0,
            stdout_tail: "Dry run: prompt written to app/BUILD-PROMPT.md. Hand it to any coding agent.".into(),
            stderr_tail: String::new(),
            files: list_files(workdir),
        })
    }
}

/// Presets for agents we know the command line of. Each one is a
/// [`Script`] template; override any with `AYSAR_AGENT_<NAME>` (upper-case,
/// dashes as underscores), e.g. `AYSAR_AGENT_AIDER="aider --model x --message-file {prompt_file} --yes-always"`.
///
/// Verified live: claude-code. The others follow the tools' documented
/// flags and are marked "untested" in `aysar doctor` until someone runs them.
pub const PRESETS: &[(&str, &str, bool)] = &[
    ("aider", "aider --message-file {prompt_file} --yes-always --no-git --no-auto-commits", false),
    ("gemini", "gemini --yolo -p {prompt}", false),
    ("opencode", "opencode run {prompt}", false),
    ("goose", "goose run --text {prompt}", false),
    ("qwen", "qwen --yolo -p {prompt}", false),
];

fn preset(name: &str) -> Option<Script> {
    let (label, template, _) = PRESETS.iter().find(|(n, _, _)| *n == name)?;
    let env_key = format!("AYSAR_AGENT_{}", label.to_ascii_uppercase().replace('-', "_"));
    let template = std::env::var(env_key).unwrap_or_else(|_| template.to_string());
    Some(Script { label: label.to_string(), template })
}

/// Choose an agent by name, or the best available one.
pub fn select(name: Option<&str>) -> anyhow::Result<Box<dyn Agent>> {
    // Names are matched case-insensitively; a command template keeps its case.
    let original = name.map(|s| s.trim().to_string());
    match original.as_deref().map(|s| s.to_ascii_lowercase()).as_deref() {
        None | Some("auto") => {
            let c = ClaudeCode::default();
            if c.available() {
                return Ok(Box::new(c));
            }
            let x = Codex::default();
            if x.available() {
                return Ok(Box::new(x));
            }
            for (n, _, _) in PRESETS {
                if let Some(p) = preset(n) {
                    if p.available() {
                        return Ok(Box::new(p));
                    }
                }
            }
            Ok(Box::new(DryRun))
        }
        Some("claude" | "claude-code") => Ok(Box::new(ClaudeCode::default())),
        Some("codex") => Ok(Box::new(Codex::default())),
        Some("dry-run" | "none") => Ok(Box::new(DryRun)),
        Some(other) => {
            if let Some(p) = preset(other) {
                return Ok(Box::new(p));
            }
            if other.contains(' ') || other.contains("{prompt") {
                Ok(Box::new(Script { label: "script".into(), template: original.unwrap_or_default() }))
            } else {
                let names: Vec<&str> = PRESETS.iter().map(|(n, _, _)| *n).collect();
                anyhow::bail!(
                    "unknown agent `{other}`; use claude-code, codex, {}, dry-run, or a command template with {{prompt}} / {{prompt_file}}",
                    names.join(", ")
                )
            }
        }
    }
}

/// One line per agent: name, installed?, verified live?
#[derive(Debug, Clone)]
pub struct AgentStatus {
    pub name: String,
    pub installed: bool,
    pub verified: bool,
    pub command: String,
}

/// All built-in agents and whether each is installed.
pub fn status() -> Vec<(String, bool)> {
    status_detailed().into_iter().map(|a| (a.name, a.installed)).collect()
}

pub fn status_detailed() -> Vec<AgentStatus> {
    let c = ClaudeCode::default();
    let x = Codex::default();
    let mut out = vec![
        AgentStatus {
            name: "claude-code".into(),
            installed: c.available(),
            verified: true,
            command: format!("{} -p <prompt> {}", c.binary, c.extra_args.join(" ")),
        },
        AgentStatus {
            name: "codex".into(),
            installed: x.available(),
            verified: false,
            command: format!("{} exec {} <prompt>", x.binary, x.extra_args.join(" ")),
        },
    ];
    for (n, _, verified) in PRESETS {
        if let Some(p) = preset(n) {
            out.push(AgentStatus {
                name: n.to_string(),
                installed: p.available(),
                verified: *verified,
                command: p.template,
            });
        }
    }
    out.push(AgentStatus {
        name: "dry-run".into(),
        installed: true,
        verified: true,
        command: "writes app/BUILD-PROMPT.md".into(),
    });
    out
}

/// Build a `Command` for `bin`, going through `cmd /C` on Windows when the
/// binary is a `.cmd`/`.bat` shim (npm installs Claude Code and Codex that way).
fn command_for(bin: &str) -> Command {
    if cfg!(windows) && !bin.contains('/') && !bin.contains('\\') {
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                for ext in [".cmd", ".bat"] {
                    let p = dir.join(format!("{bin}{ext}"));
                    if p.is_file() {
                        let mut c = Command::new("cmd");
                        c.arg("/C").arg(p);
                        return c;
                    }
                }
            }
        }
    }
    Command::new(bin)
}

fn run_command(agent: &str, mut cmd: Command, workdir: &Path) -> anyhow::Result<BuildOutput> {
    fs::create_dir_all(workdir)?;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let start = Instant::now();
    let program = cmd.get_program().to_string_lossy().to_string();
    let out = cmd.output().map_err(|e| anyhow::anyhow!("could not start {agent} (`{program}`): {e}"))?;
    Ok(BuildOutput {
        agent: agent.into(),
        ok: out.status.success(),
        seconds: start.elapsed().as_secs_f64(),
        stdout_tail: tail(&String::from_utf8_lossy(&out.stdout), 4000),
        stderr_tail: tail(&String::from_utf8_lossy(&out.stderr), 2000),
        files: list_files(workdir),
    })
}

/// List files under a folder (relative paths, skipping heavy dirs).
pub fn list_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>, depth: usize) {
        if depth > 5 || out.len() > 500 {
            return;
        }
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || name == "node_modules" || name == "target" || name == ".git" {
                    continue;
                }
                if p.is_dir() {
                    walk(&p, root, out, depth + 1);
                } else {
                    out.push(p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    walk(root, root, &mut out, 0);
    out.sort();
    out
}

fn which(bin: &str) -> bool {
    if bin.contains('/') || bin.contains('\\') {
        return Path::new(bin).exists();
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let exts: Vec<String> =
        if cfg!(windows) { vec!["".into(), ".exe".into(), ".cmd".into(), ".bat".into()] } else { vec!["".into()] };
    std::env::split_paths(&path).any(|dir| exts.iter().any(|e| dir.join(format!("{bin}{e}")).is_file()))
}

fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut start = s.len() - max;
        while !s.is_char_boundary(start) {
            start += 1;
        }
        format!("…{}", &s[start..])
    }
}

/// Minimal shell-like splitting: spaces separate, quotes group.
fn shell_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in s.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => quote = Some(ch),
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[allow(dead_code)]
fn _unused(_: PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split() {
        assert_eq!(
            shell_split(r#"aider --message "hi there" {prompt_file}"#),
            vec!["aider", "--message", "hi there", "{prompt_file}"]
        );
    }

    #[test]
    fn dry_run_writes_prompt() {
        let dir = std::env::temp_dir().join(format!("aysar-dry-{}", std::process::id()));
        let out = DryRun.run(&dir, "build it").unwrap();
        assert!(out.ok);
        assert!(dir.join("BUILD-PROMPT.md").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn select_unknown_errors() {
        assert!(select(Some("nope")).is_err());
        assert!(select(Some("dry-run")).is_ok());
        assert_eq!(select(Some("aider")).unwrap().name(), "aider");
        assert_eq!(select(Some("my-agent --file {prompt_file}")).unwrap().name(), "script");
        assert_eq!(select(Some("AIDER")).unwrap().name(), "aider");
        // A template keeps its case (an error names the binary as given).
        let t = select(Some("/Users/Me/bin/Agent --model GPT-4o {prompt_file}")).unwrap();
        let err = t.run(&std::env::temp_dir().join(format!("aysar-case-{}", std::process::id())), "x").unwrap_err();
        assert!(err.to_string().contains("/Users/Me/bin/Agent"), "{err}");
        assert!(status_detailed().iter().any(|a| a.name == "gemini"));
    }
}
