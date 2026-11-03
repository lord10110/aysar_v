//! `aysar` — the command line.

use anyhow::Context;
use aysar_core::capture::Capture;
use aysar_core::evidence::Ledger;
use aysar_core::project::Project;
use aysar_core::{Kind, Mode, Target};
use aysar_orchestrator::{RebuildOptions, Stdout};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "aysar", version, about = "See how any website, app or game works, then build your own.", long_about = None)]
struct Cli {
    /// Read everything (source, scripts, binaries). Default is Clean room.
    #[arg(long, global = true, conflicts_with = "clean_room")]
    deep: bool,
    /// Only what a visitor sees (the default).
    #[arg(long, global = true)]
    clean_room: bool,
    /// Skip Chromium rendering (no screenshots, measured styles or visual checks).
    #[arg(long, global = true)]
    no_render: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum KindArg {
    Website,
    App,
    Game,
}

impl From<KindArg> for Kind {
    fn from(k: KindArg) -> Kind {
        match k {
            KindArg::Website => Kind::Website,
            KindArg::App => Kind::App,
            KindArg::Game => Kind::Game,
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Check the install: coding agents, analysis engines, project folder.
    Doctor,
    /// Observe a target and print what was seen (no project is created).
    Study {
        /// URL or local path.
        target: String,
        #[arg(long, default_value_t = 5)]
        max_pages: usize,
        /// Write the capture as JSON here.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Say what a local file is.
    Identify { path: PathBuf },
    /// Run the whole loop: study, spec, build, check, refine.
    Rebuild {
        /// URL, local path, or `-` for none.
        target: String,
        /// What you want, in plain words.
        request: String,
        /// Coding agent: claude-code, codex, dry-run, or a command template with {prompt_file}.
        #[arg(long)]
        agent: Option<String>,
        #[arg(long, default_value_t = 2)]
        rounds: usize,
        #[arg(long, default_value_t = 85.0)]
        target_score: f32,
        #[arg(long, default_value_t = 5)]
        max_pages: usize,
        /// Override the detected kind.
        #[arg(long, value_enum)]
        kind: Option<KindArg>,
        /// Where to create the project folder (default: ~/Documents/Aysar).
        #[arg(long)]
        projects_dir: Option<PathBuf>,
        /// Extra rule for the builder (repeatable).
        #[arg(long = "rule")]
        rules: Vec<String>,
    },
    /// Generate (or regenerate) the spec for an existing project.
    Spec {
        project: PathBuf,
        #[arg(long = "rule")]
        rules: Vec<String>,
    },
    /// Run the coding agent for an existing project (one more round).
    Build {
        project: PathBuf,
        #[arg(long)]
        agent: Option<String>,
    },
    /// Check an existing project's build against its original.
    Check { project: PathBuf },
    /// Compare two captures or sites directly, without a project.
    Compare {
        /// Original: URL, local path, or a capture.json.
        original: String,
        /// Build: URL, local path, or a capture.json.
        build: String,
    },
    /// Show a project's state, score and history.
    Status { project: PathBuf },
    /// Print the evidence ledger of a project.
    Evidence {
        project: PathBuf,
        /// Only this kind: observation, inference, check, decision, action.
        #[arg(long)]
        kind: Option<String>,
    },
    /// Serve Aysar's tools over MCP (stdio) to any agent.
    Mcp,
    /// Print the JSON snippet to register Aysar as an MCP server.
    Setup,
}

fn main() {
    // Exit quietly when the reader goes away (`aysar … | head`).
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let mode = if cli.deep { Mode::Deep } else { Mode::CleanRoom };
    if cli.no_render {
        std::env::set_var("AYSAR_NO_RENDER", "1");
    }
    let rep = Stdout;

    match cli.cmd {
        Cmd::Doctor => {
            println!("aysar {}", env!("CARGO_PKG_VERSION"));
            println!("\nCoding agents (for Build):");
            for (name, ok) in aysar_build::status() {
                println!("  {} {name}", if ok { "✓" } else { "✗" });
            }
            let engine = aysar_analysis::detect();
            println!("\nAnalysis engine (for apps, games and binaries in Deep mode):");
            if engine.name() == "none" {
                println!("  ✗ none — install craft-anything-cli: https://github.com/storytold/craft-anything");
            } else {
                println!("  ✓ {} {}", engine.name(), engine.version().unwrap_or_default());
            }
            println!("\nRenderer (for screenshots, measured styles and visual checks):");
            match aysar_render::find_chrome() {
                Some(c) => println!("  ✓ Chromium at {}", c.display()),
                None => println!("  ✗ none — install Google Chrome or Chromium, or set AYSAR_CHROME=/path/to/chrome"),
            }
            println!("\nProjects folder: {}", aysar_orchestrator::default_projects_dir().display());
            println!("Mode: {mode}");
            let any_agent = aysar_build::status().iter().any(|(n, ok)| *ok && n != "dry-run");
            if !any_agent {
                println!("\nNo coding agent found. Building will run in dry-run mode and write the prompt to app/BUILD-PROMPT.md.");
            }
        }

        Cmd::Study { target, max_pages, out } => {
            let t = Target::parse(&target);
            let mut cap =
                aysar_capture::capture(&t, &aysar_capture::CaptureOptions { mode, max_pages, ..Default::default() })?;
            let shots = aysar_orchestrator::render_adhoc(&mut cap, &t, &rep)?;
            println!("{}", aysar_capture::summarise(&cap));
            if let Some(st) = cap.pages.iter().find_map(|p| p.style.as_ref()) {
                println!("\nMeasured on the rendered page:");
                if let (Some(bg), Some(fg)) = (&st.background, &st.foreground) {
                    println!("  background {bg}, text {fg}");
                }
                let cols: Vec<String> = st.colors.iter().take(8).map(|c| c.0.clone()).collect();
                let fonts: Vec<String> = st.fonts.iter().take(4).map(|f| f.0.clone()).collect();
                println!("  colours: {}", cols.join(", "));
                println!("  fonts: {}", fonts.join(", "));
                println!("  type scale: {:?}", st.font_sizes);
                println!("  screenshots: {}", shots.join("screenshots").display());
            }
            if let Some(o) = out {
                aysar_capture::save(&cap, &o)?;
                println!("\nSaved → {}", o.display());
            }
        }

        Cmd::Identify { path } => {
            println!("{}: {}", path.display(), aysar_analysis::identify(&path)?);
        }

        Cmd::Rebuild { target, request, agent, rounds, target_score, max_pages, kind, projects_dir, rules } => {
            let t = Target::parse(&target);
            let dir = projects_dir.unwrap_or_else(aysar_orchestrator::default_projects_dir);
            let mut p = aysar_orchestrator::new_project(&dir, t, &request, mode, kind.map(Into::into))?;
            println!("Project: {}", p.root.display());
            let opts = RebuildOptions {
                mode,
                max_pages,
                agent,
                rounds,
                target_score,
                extra_rules: rules,
                kind: kind.map(Into::into),
            };
            aysar_orchestrator::rebuild(&mut p, &opts, &rep)?;
        }

        Cmd::Spec { project, rules } => {
            let mut p = Project::open(&project).context("open project")?;
            if !p.capture_file().exists() {
                aysar_orchestrator::study(&mut p, 5, &rep)?;
            }
            aysar_orchestrator::specify(&mut p, rules, &rep)?;
        }

        Cmd::Build { project, agent } => {
            let mut p = Project::open(&project).context("open project")?;
            if !p.spec_file().exists() {
                anyhow::bail!("no spec yet; run `aysar spec {}` first", project.display());
            }
            let round = p.next_check_number();
            let prev = latest_report(&p)?;
            aysar_orchestrator::build(&mut p, agent.as_deref(), round, prev.as_ref(), &rep)?;
            aysar_orchestrator::check(&mut p, &rep)?;
        }

        Cmd::Check { project } => {
            let mut p = Project::open(&project).context("open project")?;
            aysar_orchestrator::check(&mut p, &rep)?;
        }

        Cmd::Compare { original, build } => {
            let a = load_or_capture(&original, mode, &rep)?;
            let b = load_or_capture(&build, Mode::Deep, &rep)?;
            let r = aysar_verify::compare(&a, &b);
            println!("{}", r.to_markdown());
        }

        Cmd::Status { project } => {
            let p = Project::open(&project).context("open project")?;
            let s = &p.state;
            println!("{} — {:?}", s.name, s.stage);
            println!("  request: {}", s.request);
            println!("  target:  {} ({})", s.target, s.kind);
            println!("  mode:    {}", s.mode);
            if let Some(a) = &s.agent {
                println!("  agent:   {a}");
            }
            if let Some(sc) = s.score {
                println!("  score:   {sc:.0}/100");
            }
            println!("  app:     {}", p.app_dir().display());
            println!("\nHistory:");
            for h in &s.history {
                println!("  {}  {:?}  {}", h.at, h.stage, h.note);
            }
            if let Ok(rd) = std::fs::read_dir(p.inbox_dir()) {
                let qs: Vec<_> = rd.flatten().collect();
                if !qs.is_empty() {
                    println!("\nOpen questions: {} in {}", qs.len(), p.inbox_dir().display());
                }
            }
        }

        Cmd::Evidence { project, kind } => {
            let p = Project::open(&project).context("open project")?;
            let ledger = Ledger::open(p.ledger_file())?;
            for e in ledger.all()? {
                if let Some(k) = &kind {
                    let ek = serde_json::to_value(e.kind)?.as_str().unwrap_or("").to_string();
                    if &ek != k {
                        continue;
                    }
                }
                println!(
                    "{}  {:<12} {:.2}  {}  ← {}",
                    e.id,
                    format!("{:?}", e.kind).to_lowercase(),
                    e.confidence,
                    e.claim,
                    e.source
                );
            }
        }

        Cmd::Mcp => aysar_mcp::serve_stdio()?,

        Cmd::Setup => {
            let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "aysar".into());
            println!(
                "Add this to your agent's MCP configuration (Claude Code: `claude mcp add aysar -- {exe} mcp`):\n"
            );
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "mcpServers": {"aysar": {"command": exe, "args": ["mcp"]}}
                }))?
            );
        }
    }
    Ok(())
}

fn load_or_capture(s: &str, mode: Mode, rep: &dyn aysar_orchestrator::Reporter) -> anyhow::Result<Capture> {
    let p = PathBuf::from(s);
    if p.is_file() && s.ends_with(".json") {
        return aysar_capture::load(&p);
    }
    let t = Target::parse(s);
    let mut cap =
        aysar_capture::capture(&t, &aysar_capture::CaptureOptions { mode, max_pages: 20, ..Default::default() })?;
    aysar_orchestrator::render_adhoc(&mut cap, &t, rep)?;
    Ok(cap)
}

fn latest_report(p: &Project) -> anyhow::Result<Option<aysar_verify::Report>> {
    let n = p.next_check_number();
    if n <= 1 {
        return Ok(None);
    }
    let path = p.checks_dir().join(format!("check-{:03}.json", n - 1));
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&std::fs::read_to_string(path)?)?))
}
