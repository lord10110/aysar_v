//! The pipeline. Each step is a function so the CLI, the MCP server and a
//! future desktop app can run them one at a time or all together.

pub mod config;
pub mod selftest;

use aysar_build::prompt::{build_prompt, PreviousCheck};
use aysar_capture::CaptureOptions;
use aysar_core::evidence::{EvidenceKind, Ledger};
use aysar_core::project::{slugify, Project, Stage};
use aysar_core::spec::Spec;
use aysar_core::{Kind, Mode, Target};
pub use aysar_verify::Report;
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
    /// More than one agent: each builds a candidate, the best one wins.
    pub agents: Vec<String>,
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
            agents: vec![],
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

    let mut program: Option<aysar_core::capture::ProgramStudy> = None;
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
                    match aysar_analysis::study::study_program(engine.as_ref(), path, project.state.mode) {
                        Ok(st) => {
                            rep.say(&format!(
                                "  {} — {} function(s), {} string(s), {} UI text sample(s), {} feature name(s); tools: {}",
                                st.what,
                                st.function_count.map(|n| n.to_string()).unwrap_or_else(|| "?".into()),
                                st.string_count.map(|n| n.to_string()).unwrap_or_else(|| "?".into()),
                                st.ui_strings.len(),
                                st.feature_names.len(),
                                st.tools_run.join(", ")
                            ));
                            ledger.record(
                                EvidenceKind::Observation,
                                format!(
                                    "program study: {} (format {:?}, {} deps, {} functions)",
                                    st.what,
                                    st.format,
                                    st.dependencies.len(),
                                    st.function_count.unwrap_or(0)
                                ),
                                format!("engine:{}", st.engine),
                                0.9,
                                vec![],
                            )?;
                            for l in &st.limitations {
                                rep.say(&format!("    ! {l}"));
                            }
                            program = Some(st);
                        }
                        Err(e) => rep.say(&format!("  Engine study failed: {e}")),
                    }
                } else if project.state.mode.is_deep() {
                    rep.say("  No analysis engine installed (install craft-anything-cli for binaries).");
                } else {
                    rep.say("  Clean room: the file's insides are not read. Use --deep to study it with the analysis engine.");
                }
            }
        }
    }

    let mut cap = aysar_capture::capture(&project.state.target, &opts)?;
    if program.is_some() {
        cap.program = program;
        // A program is not a web page; the "no .html files" note does not apply.
        cap.limitations.retain(|l| !l.contains("no .html files"));
    }
    render_capture(&mut cap, &project.state.target, &project.capture_dir(), &ledger, rep)?;
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

/// Render every captured page at the standard widths, if Chromium is on
/// this machine. Local folders are served over a loopback HTTP server so
/// `/path` links resolve. Fills `viewports` and `style` on each page and
/// writes screenshots to `<capture_dir>/screenshots/`.
pub fn render_capture(
    cap: &mut aysar_core::capture::Capture,
    target: &Target,
    capture_dir: &Path,
    ledger: &Ledger,
    rep: &dyn Reporter,
) -> anyhow::Result<()> {
    if cap.pages.is_empty() {
        return Ok(());
    }
    if std::env::var_os("AYSAR_NO_RENDER").is_some() {
        cap.limitations.push(
            "Rendering was turned off (--no-render), so no screenshots, measured styles or visual checks.".into(),
        );
        return Ok(());
    }
    let Some(renderer) = aysar_render::Renderer::launch() else {
        cap.limitations.push("No Chromium found: pages were not rendered, so no screenshots, measured styles or visual checks. Install Chrome/Chromium or set AYSAR_CHROME.".into());
        rep.say("  No Chromium found; skipping screenshots and measured styles.");
        return Ok(());
    };
    rep.say(&format!(
        "  Rendering {} page(s) at {:?}px with {}…",
        cap.pages.len(),
        aysar_render::DEFAULT_WIDTHS,
        renderer.chrome.display()
    ));

    // Where to point the browser.
    let mut server: Option<aysar_render::StaticServer> = None;
    let base: Option<String> = match target {
        Target::Website { url } => Some(url.trim_end_matches('/').to_string()),
        Target::Local { path } => {
            let p = Path::new(path);
            let dir =
                if p.is_dir() { p.to_path_buf() } else { p.parent().map(|x| x.to_path_buf()).unwrap_or_default() };
            match aysar_render::StaticServer::serve(&dir) {
                Ok(s) => {
                    let u = s.url();
                    server = Some(s);
                    Some(u)
                }
                Err(e) => {
                    cap.limitations.push(format!("Could not serve {} for rendering: {e}", dir.display()));
                    None
                }
            }
        }
        Target::None => None,
    };
    let Some(base) = base else { return Ok(()) };

    let shots = capture_dir.join("screenshots");
    for page in cap.pages.iter_mut() {
        let url = match target {
            Target::Website { .. } => {
                // Site pages keep their own absolute source URL.
                if page.source.starts_with("http") {
                    page.source.clone()
                } else {
                    format!("{base}{}", page.path)
                }
            }
            _ => format!("{base}{}", page.path),
        };
        let name = slugify(&page.path);
        let opts = aysar_render::RenderOptions {
            screenshot_dir: Some(shots.clone()),
            name: name.clone(),
            ..Default::default()
        };
        match renderer.render(&url, &opts) {
            Ok(mut r) => {
                let widths: Vec<String> = r.viewports.iter().map(|v| format!("{}×{}", v.width, v.height)).collect();
                rep.say(&format!("    {} → {}", page.path, widths.join(", ")));
                ledger.record(
                    EvidenceKind::Observation,
                    format!(
                        "rendered {} at {}; {} colours, fonts {:?}",
                        page.path,
                        widths.join(", "),
                        r.style.colors.len(),
                        r.style.fonts.iter().map(|f| &f.0).collect::<Vec<_>>()
                    ),
                    &url,
                    1.0,
                    vec![],
                )?;
                // Measured colours/fonts are what a visitor sees: allowed in any mode.
                if page.colors.is_empty() {
                    page.colors = r.style.colors.iter().map(|c| c.0.clone()).collect();
                }
                if page.fonts.is_empty() {
                    page.fonts = r.style.fonts.iter().map(|f| f.0.clone()).collect();
                }
                for v in r.viewports.iter_mut() {
                    if let Some(f) = &v.screenshot {
                        v.screenshot = Some(format!("screenshots/{f}"));
                    }
                }
                page.viewports = r.viewports;
                page.style = Some(r.style);
                if let Some(b) = &r.behavior {
                    let working = b.links.iter().filter(|l| l.1).count();
                    ledger.record(
                        EvidenceKind::Observation,
                        format!(
                            "probed {}: {}/{} links work, {} form(s), hover on {:?}, focus on {:?}",
                            page.path,
                            working,
                            b.links.len(),
                            b.forms.len(),
                            b.hover_changes,
                            b.focus_changes
                        ),
                        &url,
                        1.0,
                        vec![],
                    )?;
                }
                page.behavior = r.behavior;
            }
            Err(e) => {
                cap.limitations.push(format!("{}: could not render ({e})", page.path));
                rep.say(&format!("    {} → not rendered: {e}", page.path));
            }
        }
    }
    drop(server);
    Ok(())
}

/// Render a capture that has no project (CLI `study` / `compare`): screenshots
/// go to a scratch folder under the system temp dir.
pub fn render_adhoc(
    cap: &mut aysar_core::capture::Capture,
    target: &Target,
    rep: &dyn Reporter,
) -> anyhow::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("aysar-adhoc-{}-{}", std::process::id(), slugify(&target.to_string())));
    fs::create_dir_all(&dir)?;
    let ledger = Ledger::open(dir.join("ledger.jsonl"))?;
    render_capture(cap, target, &dir, &ledger, rep)?;
    Ok(dir)
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

/// Step 3: run the coding agent into `app/`.
pub fn build(
    project: &mut Project,
    agent_name: Option<&str>,
    round: usize,
    previous: Option<&Report>,
    rep: &dyn Reporter,
) -> anyhow::Result<aysar_build::BuildOutput> {
    let dir = project.app_dir();
    build_into(project, &dir, agent_name, round, previous, rep)
}

/// Run the coding agent into any folder (used for candidates).
pub fn build_into(
    project: &mut Project,
    app_dir: &Path,
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
    let existing = aysar_build::list_files(app_dir);
    let prev = previous.map(|r| PreviousCheck { score: r.score, mismatches: &r.mismatches });
    let prompt = build_prompt(&spec, round, prev, &existing);
    let tag = if app_dir == project.app_dir() { String::new() } else { format!("-{}", agent.name()) };
    fs::write(project.spec_dir().join(format!("prompt-round-{round}{tag}.md")), &prompt)?;
    rep.say(&format!("Building with {} (round {round}) in {}…", agent.name(), app_dir.display()));
    let out = agent.run(app_dir, &prompt)?;
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

/// Step 4: check `app/` against the original.
pub fn check(project: &mut Project, rep: &dyn Reporter) -> anyhow::Result<Report> {
    let dir = project.app_dir();
    check_dir(project, &dir, rep)
}

/// Check any folder against the original (used for candidates). The check
/// is numbered like every other check; the folder is named in the report.
pub fn check_dir(project: &mut Project, app_dir: &Path, rep: &dyn Reporter) -> anyhow::Result<Report> {
    let ledger = Ledger::open(project.ledger_file())?;
    let original = aysar_capture::load(&project.capture_file())?;
    let mut build_cap = aysar_capture::capture_local(
        app_dir,
        &CaptureOptions { mode: Mode::Deep, max_pages: original.pages.len().max(1) + 5, ..Default::default() },
    )?;
    let n = project.next_check_number();
    if original.pages.iter().any(|p| !p.viewports.is_empty()) {
        let shots_dir = project.checks_dir().join(format!("check-{n:03}-screenshots"));
        render_capture(
            &mut build_cap,
            &Target::Local { path: app_dir.display().to_string() },
            &shots_dir,
            &ledger,
            rep,
        )?;
    }
    let mut report = aysar_verify::compare(&original, &build_cap);
    if let Some(name) = selftest::run(app_dir, rep)? {
        selftest::fold_into(&mut report, name);
    }
    write_diff_images(project, &original, &build_cap, n, rep);
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

/// Original | build | difference pictures for every page and width that
/// both sides rendered, into `checks/check-NNN-screenshots/`.
fn write_diff_images(
    project: &Project,
    original: &aysar_core::capture::Capture,
    build: &aysar_core::capture::Capture,
    n: usize,
    rep: &dyn Reporter,
) {
    let out_dir = project.checks_dir().join(format!("check-{n:03}-screenshots"));
    let mut made = 0;
    for op in &original.pages {
        let Some(bp) = build.pages.iter().find(|p| p.path == op.path) else { continue };
        for ov in &op.viewports {
            let Some(bv) = bp.viewports.iter().find(|v| v.width == ov.width) else { continue };
            let (Some(oa), Some(ba)) = (&ov.screenshot, &bv.screenshot) else { continue };
            let a = project.capture_dir().join(oa);
            let b = out_dir.join(ba);
            let out = out_dir.join(format!("{}-{}-diff.png", slugify(&op.path), ov.width));
            if aysar_render::diff::side_by_side(&a, &b, &out).is_ok() {
                made += 1;
            }
        }
    }
    if made > 0 {
        rep.say(&format!("  {made} side-by-side picture(s) → {}", out_dir.display()));
    }
}

/// The whole loop.
pub fn rebuild(project: &mut Project, opts: &RebuildOptions, rep: &dyn Reporter) -> anyhow::Result<Option<Report>> {
    study(project, opts.max_pages, rep)?;
    specify(project, opts.extra_rules.clone(), rep)?;
    let mut last: Option<Report> = None;
    let mut winner: Option<String> = opts.agent.clone();
    let mut first_round = 1;
    if opts.agents.len() > 1 {
        let (name, report) = candidates(project, &opts.agents, rep)?;
        winner = Some(name);
        last = report;
        first_round = 2;
        if let Some(r) = &last {
            if r.score >= opts.target_score {
                project.advance(Stage::Ready, "best candidate met the target")?;
                rep.say(&format!("Ready: {}", project.app_dir().display()));
                return Ok(last);
            }
        }
    }
    for round in first_round..=opts.rounds.max(first_round) {
        let out = build(project, winner.as_deref(), round, last.as_ref(), rep)?;
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

/// Build one candidate per agent, check each, keep the best in `app/`.
/// Returns the winning agent's name and its report.
pub fn candidates(
    project: &mut Project,
    agents: &[String],
    rep: &dyn Reporter,
) -> anyhow::Result<(String, Option<Report>)> {
    let ledger = Ledger::open(project.ledger_file())?;
    let mut results: Vec<(String, PathBuf, Option<Report>)> = Vec::new();
    for agent in agents {
        let dir = project.root.join("candidates").join(slugify(agent));
        fs::create_dir_all(&dir)?;
        match build_into(project, &dir, Some(agent), 1, None, rep) {
            Ok(out) if out.agent == "dry-run" => {
                results.push((agent.clone(), dir, None));
            }
            Ok(_) => {
                let r = check_dir(project, &dir, rep)?;
                results.push((agent.clone(), dir, Some(r)));
            }
            Err(e) => {
                rep.say(&format!("  {agent}: {e}"));
                ledger.record(
                    EvidenceKind::Action,
                    format!("candidate {agent} failed: {e}"),
                    "aysar-orchestrator",
                    1.0,
                    vec![],
                )?;
            }
        }
    }
    let scored: Vec<&(String, PathBuf, Option<Report>)> = results.iter().filter(|r| r.2.is_some()).collect();
    let Some(best) = scored.iter().max_by(|a, b| {
        let sa = a.2.as_ref().map(|r| r.score).unwrap_or(0.0);
        let sb = b.2.as_ref().map(|r| r.score).unwrap_or(0.0);
        sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
    }) else {
        anyhow::bail!("no candidate produced a build; agents tried: {}", agents.join(", "));
    };
    let (name, dir, report) = (&best.0, &best.1, best.2.clone());
    let summary: Vec<String> = results
        .iter()
        .map(|(a, _, r)| {
            format!("{a}={}", r.as_ref().map(|r| format!("{:.0}", r.score)).unwrap_or_else(|| "n/a".into()))
        })
        .collect();
    rep.say(&format!("Candidates: {} → best is {name}", summary.join(", ")));
    // Replace app/ with the winner.
    let app = project.app_dir();
    let _ = fs::remove_dir_all(&app);
    copy_dir(dir, &app)?;
    project.state.agent = Some(name.clone());
    project.state.score = report.as_ref().map(|r| r.score);
    project.state.candidates = results.iter().map(|(a, _, r)| (a.clone(), r.as_ref().map(|r| r.score))).collect();
    project.save()?;
    ledger.record(
        EvidenceKind::Decision,
        format!("picked candidate {name} ({}) into app/", summary.join(", ")),
        "aysar-orchestrator::candidates",
        1.0,
        vec![],
    )?;
    Ok((name.clone(), report))
}

fn copy_dir(from: &Path, to: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)?.flatten() {
        let p = e.path();
        let dest = to.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &dest)?;
        } else {
            fs::copy(&p, &dest)?;
        }
    }
    Ok(())
}

/// The most recent check report of a project, if any.
pub fn latest_report(p: &Project) -> anyhow::Result<Option<Report>> {
    let n = p.next_check_number();
    if n <= 1 {
        return Ok(None);
    }
    let path = p.checks_dir().join(format!("check-{:03}.json", n - 1));
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&fs::read_to_string(path)?)?))
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
