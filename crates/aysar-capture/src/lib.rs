//! Capture: observe a target and write down what was seen.
//!
//! Today this crate understands HTML — websites over HTTP(S) and local HTML
//! files or folders. Apps and games are routed to the analysis engine
//! (`aysar-analysis`) and to future window/behaviour recorders.
//!
//! Mode matters:
//! - **Clean room** reads only what a visitor sees: title, headings, links,
//!   forms, visible text, landmarks, image count. Stylesheets and scripts
//!   are *not* read, and raw HTML is not kept.
//! - **Deep** additionally records colours and fonts from styles, the list of
//!   stylesheets and scripts, and keeps the raw HTML under `capture/raw/`.

pub mod html;
pub mod robots;

use aysar_core::capture::{Capture, PageCapture};
use aysar_core::{Mode, Target};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use url::Url;

/// Options for a capture run.
#[derive(Debug, Clone)]
pub struct CaptureOptions {
    pub mode: Mode,
    /// Maximum number of pages to follow on the same origin.
    pub max_pages: usize,
    /// Where to store raw HTML in Deep mode. `None` keeps nothing.
    pub raw_dir: Option<PathBuf>,
    pub user_agent: String,
    pub timeout_secs: u64,
    /// Pause between requests to the same site, in milliseconds.
    pub delay_ms: u64,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        CaptureOptions {
            mode: Mode::CleanRoom,
            max_pages: 5,
            raw_dir: None,
            user_agent: format!("aysar/{} (+https://github.com/aysar-dev/aysar)", env!("CARGO_PKG_VERSION")),
            timeout_secs: 20,
            delay_ms: 300,
        }
    }
}

/// Capture a target.
pub fn capture(target: &Target, opts: &CaptureOptions) -> anyhow::Result<Capture> {
    match target {
        Target::Website { url } => capture_website(url, opts),
        Target::Local { path } => capture_local(Path::new(path), opts),
        Target::None => Ok(Capture {
            target: "-".into(),
            mode: opts.mode,
            captured_at: aysar_core::now_rfc3339(),
            pages: vec![],
            limitations: vec!["No target: nothing was studied; the build follows the request alone.".into()],
        }),
    }
}

/// Crawl a website on one origin, breadth-first, up to `max_pages`.
pub fn capture_website(start: &str, opts: &CaptureOptions) -> anyhow::Result<Capture> {
    let start_url = Url::parse(start)?;
    let origin = start_url.origin();
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(opts.timeout_secs))
        .user_agent(&opts.user_agent)
        .build();

    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<Url> = VecDeque::new();
    queue.push_back(start_url.clone());
    let mut pages = Vec::new();
    let mut limitations = Vec::new();

    // Respect robots.txt for everyone (`User-agent: *`) and for us.
    let robots = {
        let mut u = start_url.clone();
        u.set_path("/robots.txt");
        u.set_query(None);
        match agent.get(u.as_str()).call() {
            Ok(r) if r.status() == 200 => robots::Rules::parse(&r.into_string().unwrap_or_default(), "aysar"),
            _ => robots::Rules::default(),
        }
    };
    let mut skipped_by_robots = 0usize;
    let mut fetched = 0usize;

    while let Some(url) = queue.pop_front() {
        if pages.len() >= opts.max_pages {
            limitations.push(format!("Stopped after {} pages (max_pages); more pages exist.", opts.max_pages));
            break;
        }
        let key = normalise(&url);
        if !seen.insert(key) {
            continue;
        }
        if !robots.allows(url.path()) {
            skipped_by_robots += 1;
            continue;
        }
        if fetched > 0 {
            // Polite: never hammer a site.
            std::thread::sleep(std::time::Duration::from_millis(opts.delay_ms.max(robots.crawl_delay_ms())));
        }
        fetched += 1;
        let resp = match agent.get(url.as_str()).call() {
            Ok(r) => r,
            Err(ureq::Error::Status(code, _)) => {
                limitations.push(format!("{url}: HTTP {code}"));
                continue;
            }
            Err(e) => {
                limitations.push(format!("{url}: {e}"));
                continue;
            }
        };
        let ctype = resp.header("content-type").unwrap_or("").to_string();
        if !ctype.contains("html") && !ctype.is_empty() {
            limitations.push(format!("{url}: skipped, content-type {ctype}"));
            continue;
        }
        let body = resp.into_string()?;
        let page = html::parse_page(&body, url.as_str(), &path_of(&url), &Some(url.clone()), opts.mode);

        if opts.mode.is_deep() {
            if let Some(dir) = &opts.raw_dir {
                fs::create_dir_all(dir)?;
                let name = format!("{}.html", &page.content_hash[..16]);
                fs::write(dir.join(name), &body)?;
            }
        }

        for link in &page.links {
            if !link.internal {
                continue;
            }
            if let Ok(next) = url.join(&link.href) {
                if next.origin() == origin && !seen.contains(&normalise(&next)) {
                    let mut next = next;
                    next.set_fragment(None);
                    queue.push_back(next);
                }
            }
        }
        pages.push(page);
    }

    if skipped_by_robots > 0 {
        limitations.push(format!("{skipped_by_robots} page(s) were skipped because robots.txt disallows them."));
    }
    if !opts.mode.is_deep() {
        limitations.push("Clean room: stylesheets and scripts were not read; colours and fonts come from the renderer when one is available.".into());
    }

    Ok(Capture {
        target: start.to_string(),
        mode: opts.mode,
        captured_at: aysar_core::now_rfc3339(),
        pages,
        limitations,
    })
}

/// Capture a local HTML file, or every `.html` file in a folder.
pub fn capture_local(path: &Path, opts: &CaptureOptions) -> anyhow::Result<Capture> {
    let mut pages = Vec::new();
    let mut limitations = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();

    if path.is_dir() {
        collect_html(path, &mut files, 0)?;
        // Root index first, then shallow before deep, then alphabetical.
        files.sort_by_key(|f| {
            let depth = f.components().count();
            let is_index = f.file_name().map(|n| n == "index.html").unwrap_or(false);
            (depth, !is_index, f.clone())
        });
        if files.is_empty() {
            limitations.push(format!(
                "{}: no .html files found; app/game analysis needs the analysis engine.",
                path.display()
            ));
        }
    } else if path.is_file() {
        files.push(path.to_path_buf());
    } else {
        anyhow::bail!("{} does not exist", path.display());
    }

    let root =
        if path.is_dir() { path.to_path_buf() } else { path.parent().map(|p| p.to_path_buf()).unwrap_or_default() };
    for f in files.iter().take(opts.max_pages.max(1)) {
        let body = fs::read_to_string(f)?;
        let rel = f.strip_prefix(&root).unwrap_or(f).to_string_lossy().replace('\\', "/");
        let page_path = if rel == "index.html" {
            "/".to_string()
        } else {
            format!("/{}", rel.trim_end_matches("index.html").trim_end_matches('/'))
        };
        let page_path = if page_path.is_empty() { "/".into() } else { page_path };
        pages.push(html::parse_page(&body, &f.display().to_string(), &page_path, &None, opts.mode));
    }
    if files.len() > opts.max_pages {
        limitations.push(format!("Only the first {} of {} HTML files were read.", opts.max_pages, files.len()));
    }

    Ok(Capture {
        target: path.display().to_string(),
        mode: opts.mode,
        captured_at: aysar_core::now_rfc3339(),
        pages,
        limitations,
    })
}

fn collect_html(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) -> anyhow::Result<()> {
    if depth > 6 {
        return Ok(());
    }
    for e in fs::read_dir(dir)?.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        if p.is_dir() {
            collect_html(&p, out, depth + 1)?;
        } else if name.ends_with(".html") || name.ends_with(".htm") {
            out.push(p);
        }
    }
    Ok(())
}

fn normalise(u: &Url) -> String {
    let mut u = u.clone();
    u.set_fragment(None);
    let s = u.to_string();
    s.trim_end_matches('/').to_string()
}

fn path_of(u: &Url) -> String {
    let p = u.path();
    if p.is_empty() {
        "/".into()
    } else {
        p.to_string()
    }
}

/// Save a capture as JSON.
pub fn save(capture: &Capture, path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(capture)?)?;
    Ok(())
}

/// Load a capture from JSON.
pub fn load(path: &Path) -> anyhow::Result<Capture> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

/// A one-paragraph, human summary of a capture.
pub fn summarise(c: &Capture) -> String {
    let mut s = format!("{} page(s) from {} in {} mode, ~{} words.", c.pages.len(), c.target, c.mode, c.total_words());
    for p in &c.pages {
        s.push_str(&format!(
            "\n  {} — {} ({} headings, {} links, {} forms, {} images)",
            p.path,
            p.title.clone().unwrap_or_else(|| "(untitled)".into()),
            p.headings.len(),
            p.links.len(),
            p.forms.len(),
            p.image_count
        ));
    }
    for l in &c.limitations {
        s.push_str(&format!("\n  ! {l}"));
    }
    s
}

#[allow(dead_code)]
fn _assert_types(_: PageCapture) {}
