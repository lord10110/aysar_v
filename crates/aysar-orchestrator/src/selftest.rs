//! Self-tests: the behaviour leg for apps and games.
//!
//! Websites are driven in a browser. Apps and games are not, so the builder
//! is asked to leave an `aysar-build.json` in the project that says how to
//! run the result and how to prove it behaves:
//!
//! ```json
//! { "run": "cargo run --release", "selftest": "cargo test --release", "notes": "…" }
//! ```
//!
//! `check` runs `selftest` with a timeout. Exit code 0 is a pass. If the
//! command prints lines starting with `PASS ` / `FAIL `, each one becomes
//! a line in the report, and the score is the pass share.

use crate::Reporter;
use aysar_verify::Report;
use serde::Deserialize;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const FILE: &str = "aysar-build.json";
const TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Default, Deserialize)]
pub struct BuildManifest {
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub selftest: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// What a self-test run produced.
#[derive(Debug, Clone)]
pub struct SelfTest {
    pub command: String,
    pub ok: bool,
    pub timed_out: bool,
    pub passes: Vec<String>,
    pub fails: Vec<String>,
    pub tail: String,
    pub seconds: f64,
}

impl SelfTest {
    /// 0.0–1.0.
    pub fn score(&self) -> f32 {
        let n = self.passes.len() + self.fails.len();
        if n > 0 {
            self.passes.len() as f32 / n as f32
        } else if self.ok {
            1.0
        } else {
            0.0
        }
    }
}

pub fn manifest(app_dir: &Path) -> Option<BuildManifest> {
    let text = std::fs::read_to_string(app_dir.join(FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Run the self-test declared in `app_dir`, if any.
pub fn run(app_dir: &Path, rep: &dyn Reporter) -> anyhow::Result<Option<SelfTest>> {
    let Some(m) = manifest(app_dir) else { return Ok(None) };
    let Some(cmd) = m.selftest.filter(|c| !c.trim().is_empty()) else { return Ok(None) };
    rep.say(&format!("  Self-test: `{cmd}`"));

    let mut child = shell(&cmd)
        .current_dir(app_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("could not start self-test `{cmd}`: {e}"))?;
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break Some(st);
        }
        if start.elapsed() > TIMEOUT {
            let _ = child.kill();
            timed_out = true;
            break None;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let mut out = String::new();
    if let Some(mut so) = child.stdout.take() {
        let _ = so.read_to_string(&mut out);
    }
    let mut err = String::new();
    if let Some(mut se) = child.stderr.take() {
        let _ = se.read_to_string(&mut err);
    }
    let _ = child.wait();

    let mut passes = Vec::new();
    let mut fails = Vec::new();
    for line in out.lines().chain(err.lines()) {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("PASS ") {
            passes.push(rest.trim().to_string());
        } else if let Some(rest) = l.strip_prefix("FAIL ") {
            fails.push(rest.trim().to_string());
        }
    }
    let ok = status.map(|s| s.success()).unwrap_or(false) && !timed_out;
    let combined = format!("{out}\n{err}");
    let tail: String =
        combined.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    let st = SelfTest { command: cmd, ok, timed_out, passes, fails, tail, seconds: start.elapsed().as_secs_f64() };
    rep.say(&format!(
        "  Self-test {} in {:.0}s ({} pass, {} fail{})",
        if st.ok { "passed" } else { "failed" },
        st.seconds,
        st.passes.len(),
        st.fails.len(),
        if st.timed_out { ", timed out" } else { "" }
    ));
    Ok(Some(st))
}

/// Put a self-test result into a report as its behaviour leg.
pub fn fold_into(report: &mut Report, st: SelfTest) {
    let score = st.score() * 100.0;
    report.behavior = Some(match report.behavior {
        Some(b) => (b + score) / 2.0,
        None => score,
    });
    if st.timed_out {
        report.mismatches.push(format!(
            "Self-test `{}` did not finish within {} seconds.",
            st.command,
            TIMEOUT.as_secs()
        ));
    } else if !st.ok && st.fails.is_empty() {
        report.mismatches.push(format!(
            "Self-test `{}` exited with an error. Last lines:\n  {}",
            st.command,
            st.tail.replace('\n', "\n  ")
        ));
    }
    for f in &st.fails {
        report.mismatches.push(format!("Self-test FAIL: {f}"));
    }
    report.limitations.retain(|l| !l.starts_with("Behaviour (links, forms"));
    report.limitations.push(format!(
        "Behaviour for this build comes from its own self-test (`{}`): {} pass, {} fail.",
        st.command,
        st.passes.len(),
        st.fails.len()
    ));
    // Recompute the headline with the new behaviour leg. With no original to
    // compare against, the self-test is the whole story.
    if report.pages.is_empty() {
        report.score = report.behavior.unwrap_or(0.0);
    } else {
        let s = report.structure / 100.0;
        let c = report.content / 100.0;
        report.score =
            aysar_verify::blend(s, c, report.visual.map(|v| v / 100.0), report.behavior.map(|b| b / 100.0)) * 100.0;
    }
}

fn shell(cmd: &str) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Quiet;

    #[test]
    fn runs_and_parses() {
        if cfg!(windows) {
            return;
        }
        let dir = std::env::temp_dir().join(format!("aysar-selftest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(FILE),
            r#"{"run":"true","selftest":"echo 'PASS moves left'; echo 'PASS scores'; echo 'FAIL game over screen'; exit 1"}"#,
        )
        .unwrap();
        let st = run(&dir, &Quiet).unwrap().unwrap();
        assert!(!st.ok);
        assert_eq!(st.passes.len(), 2);
        assert_eq!(st.fails, vec!["game over screen".to_string()]);
        assert!((st.score() - 2.0 / 3.0).abs() < 0.01);
        assert!(run(&dir.join("nope"), &Quiet).unwrap().is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
