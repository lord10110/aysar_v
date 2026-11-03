//! The pipeline. Each step is a function so the CLI, the MCP server and a
//! future desktop app can run them one at a time or all together.

use aysar_build::prompt::{build_prompt, PreviousCheck};
use aysar_capture::CaptureOptions;
use aysar_core::evidence::{EvidenceKind, Ledger};
use aysar_core::project::{slugify, Project, Stage};
use aysar_core::spec::Spec;
use aysar_core::{Kind, Mode, Target};
use aysar_verify::Report;
use std::fs;
use std::path::{Path, PathBuf};

/// Progress messages, in plain words, for whoever is watching.
pub trait Reporter {
    fn say(&self, msg: &str);
}

pub struct Stdout;
impl Reporter for Stdout {
    fn say(&self, msg: &str) {
        println!("{msg}");
    }
}

pub struct Quiet;
impl Reporter for Quiet {
    fn say(&self, _: &str) {}
}

#[derive(Debug, Clone)]
pub struct RebuildOptions {
    pub mode: Mode,
    pub max_pages: usize,
    pub agent: Option<String>,
    pub rounds: usize,
    /// Stop refining once the score reaches this.
    pub target_score: f32,
    pub extra_rules: Vec<String>,
    pub kind: Option<Kind>,
}

impl Default for RebuildOptions {
    fn default() -> Self {
        RebuildOptions {
            mode: Mode::CleanRoom,
            max_pages: 5,
            agent: None,
            rounds: 2,
            target_score: 85.0,
            extra_rules: vec![],
            kind: None,
        }
    }
}

/// Where new projects go by default.
pub fn default_projects_dir() -> PathBuf {
    if let Ok(p) = std::env::var("AYSAR_PROJECTS_DIR") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join("Documents").join("Aysar")
}

/// Create a project folder for a request.
pub fn new_project(
    projects_dir: &Path,
    target: Target,
    request: &str,
    mode: Mode,
    kind: Option<Kind>,
) -> anyhow::Result<Project> {
    let base = match &target {
        Target::Website { url } => url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or("site")
            .to_string(),
        Target::Local { path } => {
            Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "local".into())
        }
        Target::None => slugify(request),
    };
    let mut name = slugify(&base);
    let mut root = projects_dir.join(&name);
    let mut n = 2;
    while root.exists() {
        name = format!("{}-{n}", slugify(&base));
        root = projects_dir.join(&name);
        n += 1;
    }
    let mut p = Project::create(&root, &name, request, target, mode)?;
    if let Some(k) = kind {
        p.state.kind = k;
        p.save()?;
    }
    Ok(p)
}

/// Step 1: study the target and save the capture.
pub fn study(
    project: &mut Project,
    max_pages: usize,
    rep: &dyn Reporter,
) -> anyhow::Result<aysar_core::capture::Capture> {
    let ledger = Ledger::open(project.ledger_file())?;
    rep.say(&format!("Studying {} in {} mode…", project.state.target, project.state.mode));
    let opts = CaptureOptions {
        mode: project.state.mode,
        max_pages,
        raw_dir: if project.state.mode.is_deep() { Some(project.capture_dir().join("raw")) } else { None },
        ..Default::default()
    };

    if let Target::Local { path } = &project.state.target {
        let p = Path::new(path);
        if p.is_file() {
            let what = aysar_analysis::identify(p)?;
            ledger.record(
                EvidenceKind::Observation,
                format!("{} is a {}", p.display(), what),
                "aysar-analysis::identify",
                0.9,
                vec![],
            )?;
            rep.say(&format!("  {} looks like: {what}", p.display()));
            if !what.contains("HTML") {
                let engine = aysar_analysis::detect();
                if project.state.mode.is_deep() && engine.name() != "none" {
                    rep.say(&format!("  Analysis engine: {} {}", engine.name(), engine.version().unwrap_or_default()));
                    // Engine-driven app/game analysis lands here (see docs/ROADMAP.md, phase 3).
                } else if project.state.mode.is_deep() {
                    rep.say("  No analysis engine installed (install craft-anything-cli for binaries).");
                } else {
                    rep.say("  Clean room: the file's insides are not read.");
                }
            }
        }
    }

    let cap = aysar_capture::capture(&project.state.target, &opts)?;
    aysar_capture::save(&cap, &project.capture_file())?;
    for p in &cap.pages {
        ledger.record(
            EvidenceKind::Observation,
            format!(
                "captured {} ({} headings, {} links, {} forms, ~{} words)",
                p.path,
                p.headings.len(),
                p.links.len(),
                p.forms.len(),
                p.word_count
            ),
            &p.source,
            1.0,
            vec![],
        )?;
    }
    rep.say(&aysar_capture::summarise(&cap));
    project.advance(Stage::Studied, format!("{} pages captured", cap.pages.len()))?;
    Ok(cap)
}

/// Step 2: generate the spec from the capture.
pub fn specify(project: &mut Project, extra_rules: Vec<String>, rep: &dyn Reporter) -> anyhow::Result<Spec> {
    let ledger = Ledger::open(project.ledger_file())?;
    let cap = aysar_capture::load(&project.capture_file())?;
    let spec = aysar_spec::generate(
        aysar_spec::SpecInput {
            name: &project.state.name,
            request: &project.state.request,
            kind: project.state.kind,
            capture: &cap,
            extra_rules,
        },
        Some(&ledger),
    )?;
    fs::write(project.spec_file(), spec.to_json())?;
    fs::write(project.spec_markdown(), spec.to_markdown())?;
    rep.say(&format!(
        "Spec: {} page(s), {} behaviour(s) → {}",
        spec.pages.len(),
        spec.behaviors.len(),
        project.spec_markdown().display()
    ));
    project.advance(Stage::Specified, "spec generated")?;
    Ok(spec)
}

/// Step 3: run the coding agent.
pub fn build(
    project: &mut Project,
    agent_name: Option<&str>,
    round: usize,
    previous: Option<&Report>,
    rep: &dyn Reporter,
) -> anyhow::Result<aysar_build::BuildOutput> {
    let ledger = Ledger::open(project.ledger_file())?;
    let spec = Spec::from_json(&fs::read_to_string(project.spec_file())?)?;
    let agent = aysar_build::select(agent_name)?;
    if !agent.available() {
        anyhow::bail!("agent `{}` is not installed or not on PATH", agent.name());
    }
    let existing = aysar_build::list_files(&project.app_dir());
    let prev = previous.map(|r| PreviousCheck { score: r.score, mismatches: &r.mismatches });
    let prompt = build_prompt(&spec, round, prev, &existing);
    fs::write(project.spec_dir().join(format!("prompt-round-{round}.md")), &prompt)?;
    rep.say(&format!("Building with {} (round {round})…", agent.name()));
    let out = agent.run(&project.app_dir(), &prompt)?;
    ledger.record(
        EvidenceKind::Action,
        format!(
            "build round {round} with {}: {} ({} files, {:.0}s)",
            out.agent,
            if out.ok { "ok" } else { "failed" },
            out.files.len(),
            out.seconds
        ),
        format!("agent:{}", out.agent),
        1.0,
        vec![],
    )?;
    if !out.stdout_tail.trim().is_empty() {
        rep.say(&format!("  {}", last_lines(&out.stdout_tail, 6)));
    }
    if !out.ok {
        rep.say(&format!("  Agent reported failure. stderr: {}", last_lines(&out.stderr_tail, 6)));
    }
    project.state.agent = Some(out.agent.clone());
    project.advance(
        if out.ok { Stage::Built } else { Stage::Failed },
        format!("round {round}: {} files", out.files.len()),
    )?;
    Ok(out)
}

/// Step 4: check the build against the original.
pub fn check(project: &mut Project, rep: &dyn Reporter) -> anyhow::Result<Report> {
    let ledger = Ledger::open(project.ledger_file())?;
    let original = aysar_capture::load(&project.capture_file())?;
    let build_cap = aysar_capture::capture_local(
        &project.app_dir(),
        &CaptureOptions { mode: Mode::Deep, max_pages: original.pages.len().max(1) + 5, ..Default::default() },
    )?;
    let report = aysar_verify::compare(&original, &build_cap);
    let n = project.next_check_number();
    let json = project.checks_dir().join(format!("check-{n:03}.json"));
    let md = project.checks_dir().join(format!("check-{n:03}.md"));
    fs::write(&json, serde_json::to_string_pretty(&report)?)?;
    fs::write(&md, report.to_markdown())?;
    ledger.record(
        EvidenceKind::Check,
        format!("check {n}: {:.0}/100, {} mismatch(es)", report.score, report.mismatches.len()),
        md.display().to_string(),
        1.0,
        vec![],
    )?;
    rep.say(&format!(
        "Check {n}: {:.0}/100 — {} mismatch(es) → {}",
        report.score,
        report.mismatches.len(),
        md.display()
    ));
    for m in report.mismatches.iter().take(8) {
        rep.say(&format!("  - {m}"));
    }
    if report.mismatches.len() > 8 {
        rep.say(&format!("  … {} more in the report", report.mismatches.len() - 8));
    }
    project.state.score = Some(report.score);
    project.advance(Stage::Checked, format!("check {n}: {:.0}", report.score))?;
    Ok(report)
}

/// The whole loop.
pub fn rebuild(project: &mut Project, opts: &RebuildOptions, rep: &dyn Reporter) -> anyhow::Result<Option<Report>> {
    study(project, opts.max_pages, rep)?;
    specify(project, opts.extra_rules.clone(), rep)?;
    let mut last: Option<Report> = None;
    for round in 1..=opts.rounds.max(1) {
        let out = build(project, opts.agent.as_deref(), round, last.as_ref(), rep)?;
        if out.agent == "dry-run" {
            rep.say("Dry run: no agent installed. The prompt is in app/BUILD-PROMPT.md; run `aysar build` after installing Claude Code or Codex.");
            return Ok(None);
        }
        let report = check(project, rep)?;
        let score = report.score;
        last = Some(report);
        if score >= opts.target_score {
            rep.say(&format!("Reached {score:.0} ≥ target {:.0}. Ready.", opts.target_score));
            break;
        }
    }
    project.advance(Stage::Ready, "pipeline finished")?;
    rep.say(&format!("Ready: {}", project.app_dir().display()));
    Ok(last)
}

fn last_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n  ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_to_end_local_dry_run() {
        let tmp = std::env::temp_dir().join(format!("aysar-e2e-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let site = tmp.join("site");
        fs::create_dir_all(site.join("about")).unwrap();
        fs::write(site.join("index.html"), r#"<html><head><title>Demo</title></head><body><nav><a href="/about">About</a><a href="/">Home</a></nav><main><h1>Welcome</h1><p>one two three four five</p></main><footer>f</footer></body></html>"#).unwrap();
        fs::write(
            site.join("about").join("index.html"),
            r#"<html><head><title>About</title></head><body><main><h1>About us</h1><p>text</p></main></body></html>"#,
        )
        .unwrap();

        let projects = tmp.join("projects");
        let mut p = new_project(
            &projects,
            Target::parse(&site.display().to_string()),
            "Recreate this website",
            Mode::CleanRoom,
            Some(Kind::Website),
        )
        .unwrap();
        let opts = RebuildOptions { agent: Some("dry-run".into()), rounds: 1, ..Default::default() };
        let r = rebuild(&mut p, &opts, &Quiet).unwrap();
        assert!(r.is_none());
        assert!(p.spec_file().exists());
        assert!(p.app_dir().join("BUILD-PROMPT.md").exists());

        // Pretend the agent built a perfect copy, then check.
        fs::create_dir_all(p.app_dir().join("about")).unwrap();
        fs::copy(site.join("index.html"), p.app_dir().join("index.html")).unwrap();
        fs::copy(site.join("about").join("index.html"), p.app_dir().join("about").join("index.html")).unwrap();
        let report = check(&mut p, &Quiet).unwrap();
        assert!(report.score > 99.0, "{}", report.to_markdown());
        let _ = fs::remove_dir_all(&tmp);
    }
}
