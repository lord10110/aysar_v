//! Rendering: what the page *looks like*.
//!
//! Drives a headless Chromium already on the machine (never downloaded) to
//! take full-page screenshots at several widths, measure where the main
//! regions sit, and sample computed styles. All of this is what a visitor
//! sees, so it is allowed in **Clean room**; no source or script is read.
//!
//! Also provides [`StaticServer`], a tiny HTTP server for a folder, so local
//! sites and generated builds can be rendered with real `/path` links.

pub mod dhash;
pub mod diff;
pub mod probe;
pub mod server;

use aysar_core::capture::{BehaviorProbe, LayoutBox, StyleSample, Viewport};
use headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption;
use headless_chrome::{Browser, LaunchOptions};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub use server::StaticServer;

/// `AYSAR_RENDER_TRACE=1` prints how long each browser step takes, to stderr.
fn trace(step: &str, started: std::time::Instant) {
    if std::env::var_os("AYSAR_RENDER_TRACE").is_some() {
        eprintln!("[render] {step}: {:.2}s", started.elapsed().as_secs_f64());
    }
}

/// The three widths every page is measured at.
pub const DEFAULT_WIDTHS: [u32; 3] = [360, 768, 1280];

/// Find a Chromium/Chrome binary without downloading one.
pub fn find_chrome() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("AYSAR_CHROME") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    for name in ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "chrome", "msedge"] {
        if let Some(p) = which(name) {
            return Some(p);
        }
    }
    let fixed = [
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
        "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
        "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
    ];
    for f in fixed {
        let p = PathBuf::from(f);
        if p.exists() {
            return Some(p);
        }
    }
    // Playwright / Puppeteer caches.
    let mut roots: Vec<PathBuf> = vec![PathBuf::from("/opt/pw-browsers")];
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        roots.push(home.join(".cache").join("ms-playwright"));
        roots.push(home.join("Library").join("Caches").join("ms-playwright"));
        roots.push(home.join("AppData").join("Local").join("ms-playwright"));
        roots.push(home.join(".cache").join("puppeteer").join("chrome"));
    }
    for root in roots {
        if let Ok(rd) = std::fs::read_dir(&root) {
            let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
            dirs.sort();
            for d in dirs.into_iter().rev() {
                for rel in [
                    "chrome-linux64/chrome",
                    "chrome-linux/chrome",
                    "chrome-mac/Chromium.app/Contents/MacOS/Chromium",
                    "chrome-win/chrome.exe",
                    "chrome-win64/chrome.exe",
                ] {
                    let p = d.join(rel);
                    if p.exists() {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}

fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<&str> = if cfg!(windows) { vec!["", ".exe"] } else { vec![""] };
    std::env::split_paths(&path).find_map(|d| exts.iter().map(|e| d.join(format!("{bin}{e}"))).find(|p| p.is_file()))
}

/// One rendered page: a viewport record per width plus measured styles.
#[derive(Debug, Clone, Default)]
pub struct Rendered {
    pub viewports: Vec<Viewport>,
    pub style: StyleSample,
    /// Links, forms and hover/focus states exercised at the widest viewport.
    pub behavior: Option<BehaviorProbe>,
}

/// Options for rendering.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub widths: Vec<u32>,
    /// Folder to write screenshots into. `None` keeps only the hash.
    pub screenshot_dir: Option<PathBuf>,
    /// Prefix for screenshot file names (usually the page slug).
    pub name: String,
    pub timeout: Duration,
    /// Also exercise links, forms and hover/focus states (a few seconds more).
    pub probe: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            widths: DEFAULT_WIDTHS.to_vec(),
            screenshot_dir: None,
            name: "page".into(),
            timeout: Duration::from_secs(30),
            probe: true,
        }
    }
}

/// A live browser.
pub struct Renderer {
    browser: Browser,
    pub chrome: PathBuf,
}

impl Renderer {
    /// Launch headless Chromium if one is installed. `None` if not.
    pub fn launch() -> Option<Renderer> {
        let chrome = find_chrome()?;
        Self::launch_with(&chrome).ok()
    }

    pub fn launch_with(chrome: &Path) -> anyhow::Result<Renderer> {
        // Prefer Chromium's sandbox. It is refused as root (containers) and on
        // kernels that block unprivileged user namespaces (Ubuntu 24.04 CI
        // runners): there the browser may start yet never load a page. So a
        // launch only counts once a page actually renders; otherwise retry
        // without the sandbox. `AYSAR_CHROME_NO_SANDBOX=1` skips the first try.
        let skip_sandbox = std::env::var_os("AYSAR_CHROME_NO_SANDBOX").is_some();
        if !skip_sandbox {
            let t = std::time::Instant::now();
            let attempt = Self::launch_opts(chrome, true);
            trace("launch (sandboxed)", t);
            if let Ok(r) = attempt {
                let t = std::time::Instant::now();
                let ok = r.healthy();
                trace(if ok { "health check ok" } else { "health check failed" }, t);
                if ok {
                    return Ok(r);
                }
            }
        }
        let t = std::time::Instant::now();
        let r = Self::launch_opts(chrome, false)?;
        trace("launch (no sandbox)", t);
        if r.healthy() {
            Ok(r)
        } else {
            anyhow::bail!("Chromium at {} started but could not render a page", chrome.display())
        }
    }

    /// Can this browser open a page and run a line of JavaScript within a few seconds?
    fn healthy(&self) -> bool {
        let Ok(tab) = self.browser.new_tab() else { return false };
        tab.set_default_timeout(Duration::from_secs(10));
        let ok = tab
            .navigate_to("about:blank")
            .and_then(|t| t.wait_until_navigated())
            .and_then(|t| t.evaluate("1 + 1", false))
            .map(|v| v.value == Some(serde_json::json!(2)))
            .unwrap_or(false);
        let _ = tab.close(true);
        ok
    }

    fn launch_opts(chrome: &Path, sandbox: bool) -> anyhow::Result<Renderer> {
        let opts = LaunchOptions::default_builder()
            .path(Some(chrome.to_path_buf()))
            .headless(true)
            .sandbox(sandbox)
            .idle_browser_timeout(Duration::from_secs(120))
            .window_size(Some((1280, 900)))
            .args(vec![
                std::ffi::OsStr::new("--disable-gpu"),
                std::ffi::OsStr::new("--hide-scrollbars"),
                std::ffi::OsStr::new("--disable-dev-shm-usage"),
                std::ffi::OsStr::new("--no-first-run"),
            ])
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let browser =
            Browser::new(opts).map_err(|e| anyhow::anyhow!("could not start Chromium at {}: {e}", chrome.display()))?;
        Ok(Renderer { browser, chrome: chrome.to_path_buf() })
    }

    /// Render one URL at every width in `opts.widths`.
    pub fn render(&self, url: &str, opts: &RenderOptions) -> anyhow::Result<Rendered> {
        let t = std::time::Instant::now();
        let tab = self.browser.new_tab().map_err(|e| anyhow::anyhow!("{e}"))?;
        trace("new tab", t);
        tab.set_default_timeout(opts.timeout);
        let result = self.render_in_tab(&tab, url, opts);
        let _ = tab.close(true);
        result
    }

    fn render_in_tab(
        &self,
        tab: &Arc<headless_chrome::Tab>,
        url: &str,
        opts: &RenderOptions,
    ) -> anyhow::Result<Rendered> {
        let mut out = Rendered::default();
        let mut widest_sampled: u32 = 0;
        let widest = opts.widths.iter().copied().max().unwrap_or(0);

        for &width in &opts.widths {
            tab.set_bounds(headless_chrome::types::Bounds::Normal {
                left: None,
                top: None,
                width: Some(width as f64),
                height: Some(900.0),
            })
            .map_err(|e| anyhow::anyhow!("{e}"))?;
            // Emulate the viewport exactly so CSS media queries fire.
            set_metrics(tab, width, 900)?;

            let t = std::time::Instant::now();
            tab.navigate_to(url).map_err(|e| anyhow::anyhow!("navigate {url}: {e}"))?;
            tab.wait_until_navigated().map_err(|e| anyhow::anyhow!("load {url}: {e}"))?;
            trace(&format!("navigate {width}px"), t);
            // Let late layout settle (fonts, lazy content).
            std::thread::sleep(Duration::from_millis(250));

            let t = std::time::Instant::now();
            let measured: Measured = {
                let v = tab.evaluate(MEASURE_JS, false).map_err(|e| anyhow::anyhow!("measure: {e}"))?;
                let s = v.value.and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_else(|| "{}".into());
                serde_json::from_str(&s).unwrap_or_default()
            };
            trace("measure", t);
            let height = measured.height.max(1);

            // Full-page screenshot: grow the viewport to the document height.
            set_metrics(tab, width, height.min(8000))?;
            std::thread::sleep(Duration::from_millis(100));
            let t = std::time::Instant::now();
            let png = tab
                .capture_screenshot(CaptureScreenshotFormatOption::Png, None, None, true)
                .map_err(|e| anyhow::anyhow!("screenshot: {e}"))?;
            trace("screenshot", t);

            let dhash = dhash::dhash_png(&png).ok();
            let screenshot = if let Some(dir) = &opts.screenshot_dir {
                std::fs::create_dir_all(dir)?;
                let file = format!("{}-{}.png", opts.name, width);
                std::fs::write(dir.join(&file), &png)?;
                Some(file)
            } else {
                None
            };

            out.viewports.push(Viewport {
                width,
                height,
                screenshot,
                dhash,
                boxes: measured
                    .boxes
                    .iter()
                    .map(|b| LayoutBox { kind: b.kind.clone(), x: b.x, y: b.y, w: b.w, h: b.h })
                    .collect(),
            });

            // Styles are sampled at the widest viewport: the desktop design is canonical.
            if width >= widest_sampled {
                out.style = to_style_sample(&measured);
                widest_sampled = width;
                // Probe exactly once, at the widest viewport.
                if opts.probe && width == widest {
                    // Back to a normal viewport so "in view" means something for hover targets.
                    set_metrics(tab, width, 900)?;
                    std::thread::sleep(Duration::from_millis(80));
                    let t = std::time::Instant::now();
                    out.behavior = probe::probe(tab).ok();
                    trace("probe", t);
                }
            }
        }
        Ok(out)
    }
}

fn set_metrics(tab: &Arc<headless_chrome::Tab>, width: u32, height: u32) -> anyhow::Result<()> {
    tab.call_method(headless_chrome::protocol::cdp::Emulation::SetDeviceMetricsOverride {
        width,
        height,
        device_scale_factor: 1.0,
        // Never mobile emulation: it would lay out at 980px when a page has no
        // viewport meta. We want the CSS-pixel width we asked for.
        mobile: false,
        scale: None,
        screen_width: None,
        screen_height: None,
        position_x: None,
        position_y: None,
        dont_set_visible_size: None,
        screen_orientation: None,
        viewport: None,
        display_feature: None,
        device_posture: None,
    })
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

#[derive(Debug, Default, Deserialize)]
struct MeasuredBox {
    kind: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[derive(Debug, Default, Deserialize)]
struct Measured {
    #[serde(default)]
    height: u32,
    #[serde(default)]
    boxes: Vec<MeasuredBox>,
    #[serde(default)]
    colors: BTreeMap<String, u32>,
    #[serde(default)]
    fonts: BTreeMap<String, u32>,
    #[serde(default)]
    font_sizes: Vec<f32>,
    #[serde(default)]
    spacing: Vec<f32>,
    #[serde(default)]
    radii: Vec<f32>,
    #[serde(default)]
    background: Option<String>,
    #[serde(default)]
    foreground: Option<String>,
}

fn to_style_sample(m: &Measured) -> StyleSample {
    let mut colors: Vec<(String, u32)> = m.colors.iter().map(|(k, v)| (k.clone(), *v)).collect();
    colors.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    colors.truncate(16);
    let mut fonts: Vec<(String, u32)> = m.fonts.iter().map(|(k, v)| (k.clone(), *v)).collect();
    fonts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    fonts.truncate(8);
    StyleSample {
        colors,
        fonts,
        font_sizes: sorted_unique(&m.font_sizes, 12),
        spacing: sorted_unique(&m.spacing, 12),
        radii: sorted_unique(&m.radii, 8),
        background: m.background.clone(),
        foreground: m.foreground.clone(),
    }
}

fn sorted_unique(v: &[f32], max: usize) -> Vec<f32> {
    let mut out: Vec<f32> = Vec::new();
    let mut vals: Vec<f32> = v.iter().copied().filter(|x| x.is_finite() && *x >= 0.0).collect();
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    for x in vals {
        if out.last().map(|l| (l - x).abs() > 0.5).unwrap_or(true) {
            out.push((x * 10.0).round() / 10.0);
        }
    }
    out.truncate(max);
    out
}

/// Runs in the page. Returns a JSON string so the result crosses CDP as one value.
const MEASURE_JS: &str = r#"
(function () {
  function hex(c) {
    const m = c && c.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/);
    if (!m) return null;
    if (m[4] !== undefined && parseFloat(m[4]) === 0) return null;
    const h = n => ('0' + parseInt(n).toString(16)).slice(-2);
    return '#' + h(m[1]) + h(m[2]) + h(m[3]);
  }
  const colors = {}, fonts = {}, sizes = {}, spacing = {}, radii = {};
  const bump = (o, k) => { if (k) o[k] = (o[k] || 0) + 1; };
  const px = v => { const n = parseFloat(v); return isNaN(n) ? null : n; };
  const all = document.body ? document.body.querySelectorAll('*') : [];
  let n = 0, maxBottom = 0;
  for (const el of all) {
    if (n++ > 3000) break;
    const r = el.getBoundingClientRect();
    if (r.width === 0 || r.height === 0) continue;
    if (r.bottom + scrollY > maxBottom) maxBottom = r.bottom + scrollY;
    const cs = getComputedStyle(el);
    if (el.textContent && el.textContent.trim()) bump(colors, hex(cs.color));
    bump(colors, hex(cs.backgroundColor));
    const fam = (cs.fontFamily || '').split(',')[0].trim().replace(/^["']|["']$/g, '');
    if (fam && el.textContent && el.textContent.trim()) bump(fonts, fam);
    const fs = px(cs.fontSize); if (fs) sizes[fs] = 1;
    for (const k of ['marginTop','marginBottom','paddingTop','paddingBottom','paddingLeft','paddingRight','gap','rowGap','columnGap']) {
      const v = px(cs[k]); if (v && v > 0 && v < 400) spacing[v] = 1;
    }
    const br = px(cs.borderTopLeftRadius); if (br && br > 0 && br < 200) radii[br] = 1;
  }
  const boxes = [];
  const sel = 'header, nav, main, footer, aside, h1, h2, form, img, section, article';
  const seen = {};
  for (const el of document.querySelectorAll(sel)) {
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) continue;
    const kind = el.tagName.toLowerCase();
    seen[kind] = (seen[kind] || 0) + 1;
    if (seen[kind] > 8) continue;
    boxes.push({ kind, x: Math.round(r.left + scrollX), y: Math.round(r.top + scrollY), w: Math.round(r.width), h: Math.round(r.height) });
  }
  const bcs = document.body ? getComputedStyle(document.body) : null;
  const hcs = getComputedStyle(document.documentElement);
  let background = bcs ? hex(bcs.backgroundColor) : null;
  if (!background) background = hex(hcs.backgroundColor) || '#ffffff';
  const foreground = bcs ? hex(bcs.color) : null;
  bump(colors, background);
  // Content height, not viewport height: the html element's own height or the lowest element.
  const height = Math.ceil(Math.max(document.documentElement.offsetHeight, maxBottom, 1));
  const num = o => Object.keys(o).map(Number);
  return JSON.stringify({ height, boxes, colors, fonts, font_sizes: num(sizes), spacing: num(spacing), radii: num(radii), background, foreground });
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_sorting() {
        assert_eq!(sorted_unique(&[16.0, 8.0, 16.2, 24.0, -1.0], 10), vec![8.0, 16.0, 24.0]);
    }

    /// Runs only when a Chromium is on this machine.
    #[test]
    fn render_local_page_if_chrome() {
        let Some(r) = Renderer::launch() else {
            eprintln!("no chromium; skipping");
            return;
        };
        let dir = std::env::temp_dir().join(format!("aysar-render-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("index.html"),
            r#"<html><head><style>body{margin:0;background:#fafafa;color:#222;font-family:Arial} header{height:60px;background:#123456} nav a{color:#fff} nav a:hover{color:#ff0} main{padding:24px} @media (max-width:600px){main{padding:8px}}</style></head>
            <body><header><nav><a href="/">Home</a><a href="/missing">Gone</a></nav></header><main><h1 style="font-size:32px;color:#ff0000">Hi</h1><p>text</p>
            <form><input name="email" type="email" required><button type="submit">Go</button></form></main><footer style="height:40px"></footer></body></html>"#,
        )
        .unwrap();
        let server = StaticServer::serve(&dir).unwrap();
        let opts = RenderOptions {
            widths: vec![360, 1280],
            screenshot_dir: Some(dir.join("shots")),
            name: "home".into(),
            ..Default::default()
        };
        let out = r.render(&format!("{}/", server.url()), &opts).unwrap();
        assert_eq!(out.viewports.len(), 2);
        assert!(out.viewports[0].dhash.is_some());
        assert!(dir.join("shots").join("home-360.png").exists());
        let header = out.viewports[1].boxes.iter().find(|b| b.kind == "header").unwrap();
        assert_eq!(header.h as i32, 60);
        assert_eq!(header.w as i32, 1280);
        assert!(out.style.colors.iter().any(|(c, _)| c == "#123456"));
        assert!(out.style.font_sizes.contains(&32.0));
        assert!(out.style.spacing.contains(&24.0));
        assert_eq!(out.style.background.as_deref(), Some("#fafafa"));
        let b = out.behavior.expect("probe ran");
        assert_eq!(b.links.len(), 2, "{b:?}");
        assert!(b.links.iter().any(|(h, ok)| h == "/" && *ok));
        assert!(b.links.iter().any(|(h, ok)| h == "/missing" && !*ok));
        assert_eq!(b.forms.len(), 1);
        assert_eq!(b.forms[0].required_fields, 1);
        assert!(b.forms[0].blocks_empty_submit);
        assert!(b.forms[0].accepts_filled);
        assert!(b.forms[0].has_submit);
        assert!(b.hover_changes.contains(&"nav a".to_string()), "{b:?}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
