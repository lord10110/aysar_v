//! Verification: compare what was built with what was observed.
//!
//! The score is composite: **structure** (pages, headings, landmarks,
//! navigation, forms), **content shape** (word counts, image counts) and,
//! when both sides were rendered, **visual** (screenshot difference hash and
//! region layout at each width). A **behaviour** leg (driving both sites and
//! comparing state machines) is planned and has a reserved field.
//!
//! Every mismatch is a plain sentence the builder can act on.

use aysar_core::capture::{Capture, PageCapture};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageReport {
    pub path: String,
    pub found: bool,
    pub title_match: f32,
    pub headings: f32,
    pub landmarks: f32,
    pub navigation: f32,
    pub forms: f32,
    pub content_shape: f32,
    /// Visual similarity 0–1 when both sides were rendered.
    #[serde(default)]
    pub visual: Option<f32>,
    pub score: f32,
    pub mismatches: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub checked_at: String,
    pub original: String,
    pub build: String,
    /// 0–100.
    pub score: f32,
    pub structure: f32,
    pub content: f32,
    /// 0–100 when both captures have rendered viewports.
    pub visual: Option<f32>,
    /// Reserved: `None` until behaviour checks are wired in.
    pub behavior: Option<f32>,
    pub pages: Vec<PageReport>,
    pub mismatches: Vec<String>,
    pub limitations: Vec<String>,
}

impl Report {
    pub fn to_markdown(&self) -> String {
        let mut s = format!("# Check — {:.0}/100\n\n", self.score);
        s.push_str(&format!(
            "- Original: {}\n- Build: {}\n- Structure: {:.0} · Content: {:.0}",
            self.original, self.build, self.structure, self.content
        ));
        if let Some(v) = self.visual {
            s.push_str(&format!(" · Visual: {v:.0}"));
        }
        if let Some(b) = self.behavior {
            s.push_str(&format!(" · Behaviour: {b:.0}"));
        }
        s.push_str("\n\n## Pages\n\n| Page | Score | Title | Headings | Landmarks | Nav | Forms | Content | Visual |\n|---|---|---|---|---|---|---|---|---|\n");
        for p in &self.pages {
            if p.found {
                s.push_str(&format!(
                    "| `{}` | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {} |\n",
                    p.path,
                    p.score,
                    p.title_match * 100.0,
                    p.headings * 100.0,
                    p.landmarks * 100.0,
                    p.navigation * 100.0,
                    p.forms * 100.0,
                    p.content_shape * 100.0,
                    p.visual.map(|v| format!("{:.0}", v * 100.0)).unwrap_or_else(|| "–".into())
                ));
            } else {
                s.push_str(&format!("| `{}` | 0 | missing | | | | | | |\n", p.path));
            }
        }
        if !self.mismatches.is_empty() {
            s.push_str("\n## What doesn't match yet\n\n");
            for m in &self.mismatches {
                s.push_str(&format!("- {m}\n"));
            }
        } else {
            s.push_str("\n_No mismatches found at this level of checking._\n");
        }
        if !self.limitations.is_empty() {
            s.push_str("\n## Not checked\n\n");
            for l in &self.limitations {
                s.push_str(&format!("- {l}\n"));
            }
        }
        s
    }
}

/// Weights when a visual leg is present. Structure + content + visual = 1.
pub const WEIGHT_STRUCTURE_WITH_VISUAL: f32 = 0.40;
pub const WEIGHT_CONTENT_WITH_VISUAL: f32 = 0.10;
pub const WEIGHT_VISUAL: f32 = 0.50;

/// Compare a build capture against the original capture.
pub fn compare(original: &Capture, build: &Capture) -> Report {
    let mut pages = Vec::new();
    let mut mismatches = Vec::new();

    for op in &original.pages {
        let bp = find_page(build, &op.path);
        match bp {
            None => {
                mismatches.push(format!("Page `{}` is missing from the build.", op.path));
                pages.push(PageReport {
                    path: op.path.clone(),
                    found: false,
                    title_match: 0.0,
                    headings: 0.0,
                    landmarks: 0.0,
                    navigation: 0.0,
                    forms: 0.0,
                    content_shape: 0.0,
                    visual: None,
                    score: 0.0,
                    mismatches: vec![],
                });
            }
            Some(bp) => {
                let r = compare_page(op, bp);
                mismatches.extend(r.mismatches.iter().cloned());
                pages.push(r);
            }
        }
    }

    for bp in &build.pages {
        if find_page(original, &bp.path).is_none() && !original.pages.is_empty() {
            mismatches.push(format!("Build has an extra page `{}` that the original does not.", bp.path));
        }
    }

    let n = pages.len().max(1) as f32;
    let structure =
        pages.iter().map(|p| (p.title_match + p.headings + p.landmarks + p.navigation + p.forms) / 5.0).sum::<f32>()
            / n
            * 100.0;
    let content = pages.iter().map(|p| p.content_shape).sum::<f32>() / n * 100.0;
    // Visual: mean over pages that have it; a missing page counts as 0 when
    // the original was rendered at all.
    let any_rendered = original.pages.iter().any(|p| !p.viewports.is_empty());
    let visual = if any_rendered {
        let total: f32 = pages.iter().map(|p| p.visual.unwrap_or(0.0)).sum();
        Some(total / n * 100.0)
    } else {
        None
    };
    let score = if original.pages.is_empty() {
        // Nothing to compare with: report what exists.
        if build.pages.is_empty() {
            0.0
        } else {
            50.0
        }
    } else {
        match visual {
            Some(v) => {
                structure * WEIGHT_STRUCTURE_WITH_VISUAL + content * WEIGHT_CONTENT_WITH_VISUAL + v * WEIGHT_VISUAL
            }
            None => structure * 0.7 + content * 0.3,
        }
    };

    let mut limitations = vec!["Behaviour (clicking, submitting, navigating) is not checked yet.".into()];
    if visual.is_none() {
        limitations.push(
            "Visual similarity was not checked: no renderer (Chromium) was available when the original was studied."
                .into(),
        );
    } else if build.pages.iter().all(|p| p.viewports.is_empty()) {
        limitations.push("The build was not rendered, so its visual score is 0; is Chromium still installed?".into());
    }
    if original.pages.is_empty() {
        limitations.push("The original had no captured pages, so only the build's existence was checked.".into());
    }

    Report {
        checked_at: aysar_core::now_rfc3339(),
        original: original.target.clone(),
        build: build.target.clone(),
        score: score.clamp(0.0, 100.0),
        structure,
        content,
        visual,
        behavior: None,
        pages,
        mismatches,
        limitations,
    }
}

fn find_page<'a>(c: &'a Capture, path: &str) -> Option<&'a PageCapture> {
    let norm = |p: &str| p.trim_end_matches('/').trim_end_matches(".html").trim_end_matches("/index").to_string();
    let want = norm(path);
    c.pages.iter().find(|p| norm(&p.path) == want || (want.is_empty() && norm(&p.path).is_empty()))
}

fn compare_page(o: &PageCapture, b: &PageCapture) -> PageReport {
    let mut mm = Vec::new();

    let title_match = match (&o.title, &b.title) {
        (Some(ot), Some(bt)) => {
            let s = token_similarity(ot, bt);
            if s < 0.5 {
                mm.push(format!("`{}`: title differs ({:?} vs {:?}).", o.path, ot, bt));
            }
            s
        }
        (None, None) => 1.0,
        (Some(ot), None) => {
            mm.push(format!("`{}`: build page has no <title>; the original is {:?}.", o.path, ot));
            0.0
        }
        (None, Some(_)) => 0.8,
    };

    // Headings: the *structure* (h1, h2, h2, …) matters most, because the
    // builder is told not to copy text. Same words are a small bonus.
    let ol_levels: Vec<String> = o.headings.iter().map(|h| format!("h{}", h.level)).collect();
    let bl_levels: Vec<String> = b.headings.iter().map(|h| format!("h{}", h.level)).collect();
    let oh: Vec<String> = o.headings.iter().map(|h| format!("h{}:{}", h.level, norm(&h.text))).collect();
    let bh: Vec<String> = b.headings.iter().map(|h| format!("h{}:{}", h.level, norm(&h.text))).collect();
    let level_sim =
        if ol_levels.is_empty() && bl_levels.is_empty() { 1.0 } else { seq_similarity(&ol_levels, &bl_levels) };
    let text_sim = if oh.is_empty() && bh.is_empty() { 1.0 } else { seq_similarity(&oh, &bh) };
    let headings = 0.8 * level_sim + 0.2 * text_sim;
    if level_sim < 0.6 {
        let o_list: Vec<String> = o.headings.iter().take(6).map(|h| format!("h{} \"{}\"", h.level, h.text)).collect();
        mm.push(format!(
            "`{}`: heading structure differs; the original has {} ({}{}) and the build has {} ({}).",
            o.path,
            o.headings.len(),
            o_list.join(", "),
            if o.headings.len() > 6 { ", …" } else { "" },
            b.headings.len(),
            bl_levels.join(", ")
        ));
    }

    let ol: HashSet<String> = o.landmarks.iter().map(|l| l.split('×').next().unwrap_or(l).to_string()).collect();
    let bl: HashSet<String> = b.landmarks.iter().map(|l| l.split('×').next().unwrap_or(l).to_string()).collect();
    let landmarks = if ol.is_empty() { 1.0 } else { jaccard(&ol, &bl) };
    let missing: Vec<&String> = ol.iter().filter(|l| !bl.contains(*l)).collect();
    if !missing.is_empty() {
        let mut names: Vec<String> = missing.iter().map(|s| format!("<{s}>")).collect();
        names.sort();
        mm.push(format!("`{}`: missing landmarks {}.", o.path, names.join(", ")));
    }

    let on: HashSet<String> = o.links.iter().filter(|l| l.internal).map(|l| norm_path(&l.href)).collect();
    let bn: HashSet<String> = b.links.iter().filter(|l| l.internal).map(|l| norm_path(&l.href)).collect();
    let navigation = if on.is_empty() { 1.0 } else { jaccard(&on, &bn) };
    if navigation < 0.5 {
        let mut want: Vec<&String> = on.iter().filter(|p| !bn.contains(*p)).take(10).collect();
        want.sort();
        if !want.is_empty() {
            mm.push(format!(
                "`{}`: navigation is missing links to {}.",
                o.path,
                want.iter().map(|s| format!("`{s}`")).collect::<Vec<_>>().join(", ")
            ));
        }
    }

    let forms = if o.forms.is_empty() && b.forms.is_empty() {
        1.0
    } else if o.forms.len() != b.forms.len() {
        mm.push(format!("`{}`: the original has {} form(s), the build has {}.", o.path, o.forms.len(), b.forms.len()));
        (1.0 - (o.forms.len() as f32 - b.forms.len() as f32).abs() / o.forms.len().max(1) as f32).max(0.0) * 0.5
    } else {
        let mut total = 0.0;
        for (of, bf) in o.forms.iter().zip(b.forms.iter()) {
            let ofs: HashSet<String> =
                of.fields.iter().map(|f| format!("{}:{}", f.name.to_ascii_lowercase(), f.input_type)).collect();
            let bfs: HashSet<String> =
                bf.fields.iter().map(|f| format!("{}:{}", f.name.to_ascii_lowercase(), f.input_type)).collect();
            let s = if ofs.is_empty() { 1.0 } else { jaccard(&ofs, &bfs) };
            if s < 0.7 {
                let mut want: Vec<&String> = ofs.iter().filter(|f| !bfs.contains(*f)).collect();
                want.sort();
                mm.push(format!(
                    "`{}`: form is missing fields {}.",
                    o.path,
                    want.iter().map(|s| format!("`{s}`")).collect::<Vec<_>>().join(", ")
                ));
            }
            total += s;
        }
        total / o.forms.len() as f32
    };

    let words = ratio(o.word_count, b.word_count);
    let images = ratio(o.image_count, b.image_count);
    let content_shape = (words + images) / 2.0;
    if words < 0.4 {
        mm.push(format!("`{}`: the original has ~{} words, the build has ~{}.", o.path, o.word_count, b.word_count));
    }
    if images < 0.4 && o.image_count > 0 {
        mm.push(format!("`{}`: the original has {} images, the build has {}.", o.path, o.image_count, b.image_count));
    }

    let visual = compare_visual(o, b, &mut mm);
    let structural = (title_match + headings + landmarks + navigation + forms) / 5.0;
    let score = match visual {
        Some(v) => {
            (structural * WEIGHT_STRUCTURE_WITH_VISUAL + content_shape * WEIGHT_CONTENT_WITH_VISUAL + v * WEIGHT_VISUAL)
                * 100.0
        }
        None => (structural * 0.7 + content_shape * 0.3) * 100.0,
    };
    PageReport {
        path: o.path.clone(),
        found: true,
        title_match,
        headings,
        landmarks,
        navigation,
        forms,
        content_shape,
        visual,
        score,
        mismatches: mm,
    }
}

/// Visual leg for one page: per width, blend the screenshot hash
/// similarity, how well the main regions line up, and page height; then
/// blend in the palette and fonts measured on the rendered page.
fn compare_visual(o: &PageCapture, b: &PageCapture, mm: &mut Vec<String>) -> Option<f32> {
    if o.viewports.is_empty() {
        return None;
    }
    if b.viewports.is_empty() {
        mm.push(format!("`{}`: the build could not be rendered, so its look was not compared.", o.path));
        return Some(0.0);
    }
    // Same sentence at several widths → one line listing the widths.
    let mut grouped: Vec<(String, Vec<u32>)> = Vec::new();
    let mut note = |msg: String, width: u32| {
        if let Some(g) = grouped.iter_mut().find(|g| g.0 == msg) {
            g.1.push(width);
        } else {
            grouped.push((msg, vec![width]));
        }
    };

    let mut total = 0.0;
    let mut count = 0;
    for ov in &o.viewports {
        let Some(bv) = b.viewports.iter().find(|v| v.width == ov.width) else { continue };
        count += 1;

        let hash_sim = match (&ov.dhash, &bv.dhash) {
            (Some(a), Some(c)) => hash_similarity(a, c),
            _ => 0.5,
        };

        let height_ratio = ratio(ov.height as usize, bv.height as usize);
        if height_ratio < 0.6 {
            note(format!("the original page is {}px tall, the build is {}px", ov.height, bv.height), ov.width);
        }

        let mut region_scores = Vec::new();
        for kind in ["header", "nav", "main", "footer", "h1", "form"] {
            let Some(ob) = ov.boxes.iter().find(|x| x.kind == kind) else { continue };
            match bv.boxes.iter().find(|x| x.kind == kind) {
                None => region_scores.push(0.0),
                Some(bb) => {
                    let oh = ov.height.max(1) as f32;
                    let bh = bv.height.max(1) as f32;
                    let on = aysar_core::capture::LayoutBox {
                        kind: kind.into(),
                        x: ob.x / ov.width as f32,
                        y: ob.y / oh,
                        w: ob.w / ov.width as f32,
                        h: ob.h / oh,
                    };
                    let bn = aysar_core::capture::LayoutBox {
                        kind: kind.into(),
                        x: bb.x / bv.width as f32,
                        y: bb.y / bh,
                        w: bb.w / bv.width as f32,
                        h: bb.h / bh,
                    };
                    let iou = on.iou(&bn);
                    let size = ratio(ob.h as usize, bb.h as usize);
                    region_scores.push(0.5 * iou + 0.5 * size);
                    if size < 0.5 && (ob.h as i32 - bb.h as i32).abs() > 40 {
                        note(
                            format!(
                                "<{}> is {}px tall in the original and {}px in the build",
                                kind, ob.h as i32, bb.h as i32
                            ),
                            ov.width,
                        );
                    }
                }
            }
        }
        let region_sim = if region_scores.is_empty() {
            hash_sim
        } else {
            region_scores.iter().sum::<f32>() / region_scores.len() as f32
        };

        if hash_sim < 0.6 {
            note(format!("the screenshot looks different from the original (similarity {:.0}%); compare the screenshots side by side", hash_sim * 100.0), ov.width);
        }
        total += 0.4 * hash_sim + 0.3 * region_sim + 0.3 * height_ratio;
    }
    if count == 0 {
        mm.push(format!("`{}`: no matching widths were rendered on both sides.", o.path));
        return Some(0.0);
    }
    let layout = total / count as f32;

    for (msg, widths) in grouped {
        let w: Vec<String> = widths.iter().map(|x| x.to_string()).collect();
        mm.push(format!("`{}` at {}px: {}.", o.path, w.join("/"), msg));
    }

    // Palette and fonts.
    let style = match (&o.style, &b.style) {
        (Some(os), Some(bs)) => {
            let (sim, why) = style_similarity(os, bs);
            if sim < 0.6 {
                mm.push(format!("`{}`: {}", o.path, why));
            }
            sim
        }
        _ => layout,
    };

    Some(0.7 * layout + 0.3 * style)
}

/// How alike two measured style samples are, and why not, in one sentence.
fn style_similarity(o: &aysar_core::capture::StyleSample, b: &aysar_core::capture::StyleSample) -> (f32, String) {
    let mut parts: Vec<f32> = Vec::new();
    let mut why: Vec<String> = Vec::new();

    let bg = color_close(o.background.as_deref(), b.background.as_deref());
    let fg = color_close(o.foreground.as_deref(), b.foreground.as_deref());
    parts.push(bg);
    parts.push(fg);
    if bg < 0.5 || fg < 0.5 {
        why.push(format!(
            "page background/text are {}/{} in the original and {}/{} in the build",
            o.background.as_deref().unwrap_or("?"),
            o.foreground.as_deref().unwrap_or("?"),
            b.background.as_deref().unwrap_or("?"),
            b.foreground.as_deref().unwrap_or("?")
        ));
    }

    // Top colours of the original: does each have a close match in the build?
    let top: Vec<&str> = o.colors.iter().take(6).map(|c| c.0.as_str()).collect();
    if !top.is_empty() {
        let bcols: Vec<&str> = b.colors.iter().map(|c| c.0.as_str()).collect();
        let mut hit = 0;
        let mut missing = Vec::new();
        for c in &top {
            if bcols.iter().any(|x| color_distance(c, x) < 48.0) {
                hit += 1;
            } else {
                missing.push(*c);
            }
        }
        parts.push(hit as f32 / top.len() as f32);
        if !missing.is_empty() && missing.len() * 2 > top.len() {
            why.push(format!("the original's main colours {} do not appear in the build", missing.join(", ")));
        }
    }

    // Primary font family.
    if let (Some(of), _) = (o.fonts.first(), b.fonts.first()) {
        let ok = b.fonts.iter().take(3).any(|f| f.0.eq_ignore_ascii_case(&of.0));
        parts.push(if ok { 1.0 } else { 0.0 });
        if !ok {
            why.push(format!(
                "the main font is \"{}\" in the original and \"{}\" in the build",
                of.0,
                b.fonts.first().map(|f| f.0.as_str()).unwrap_or("?")
            ));
        }
    }

    let sim = if parts.is_empty() { 1.0 } else { parts.iter().sum::<f32>() / parts.len() as f32 };
    let why = if why.is_empty() { "styles match".into() } else { format!("{}.", why.join("; ")) };
    (sim, why)
}

fn parse_hex(c: &str) -> Option<(f32, f32, f32)> {
    let h = c.trim().trim_start_matches('#');
    let (r, g, b) = match h.len() {
        6 | 8 => (
            u8::from_str_radix(&h[0..2], 16).ok()?,
            u8::from_str_radix(&h[2..4], 16).ok()?,
            u8::from_str_radix(&h[4..6], 16).ok()?,
        ),
        3 | 4 => {
            let d = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).ok().map(|v| v * 17);
            (d(0)?, d(1)?, d(2)?)
        }
        _ => return None,
    };
    Some((r as f32, g as f32, b as f32))
}

/// Euclidean RGB distance (0–441).
fn color_distance(a: &str, b: &str) -> f32 {
    match (parse_hex(a), parse_hex(b)) {
        (Some(x), Some(y)) => ((x.0 - y.0).powi(2) + (x.1 - y.1).powi(2) + (x.2 - y.2).powi(2)).sqrt(),
        _ => 441.0,
    }
}

/// 1.0 for the same colour, falling to 0.0 at a distance of ~120.
fn color_close(a: Option<&str>, b: Option<&str>) -> f32 {
    match (a, b) {
        (Some(a), Some(b)) => (1.0 - color_distance(a, b) / 120.0).clamp(0.0, 1.0),
        (None, None) => 1.0,
        _ => 0.0,
    }
}

/// Hamming similarity of two 64-bit hex hashes.
fn hash_similarity(a: &str, b: &str) -> f32 {
    match (u64::from_str_radix(a, 16), u64::from_str_radix(b, 16)) {
        (Ok(x), Ok(y)) => 1.0 - (x ^ y).count_ones() as f32 / 64.0,
        _ => 0.5,
    }
}

fn norm(s: &str) -> String {
    s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn norm_path(href: &str) -> String {
    let h = href.split(['?', '#']).next().unwrap_or(href);
    let h = h.trim_start_matches("./");
    let h = h.trim_end_matches("index.html").trim_end_matches(".html").trim_end_matches('/');
    if h.is_empty() {
        "/".into()
    } else if h.starts_with('/') {
        h.to_string()
    } else {
        format!("/{h}")
    }
}

/// Similarity of two strings by word-set overlap.
pub fn token_similarity(a: &str, b: &str) -> f32 {
    let ta: HashSet<String> = norm(a).split_whitespace().map(|s| s.to_string()).collect();
    let tb: HashSet<String> = norm(b).split_whitespace().map(|s| s.to_string()).collect();
    if ta.is_empty() && tb.is_empty() {
        return 1.0;
    }
    jaccard(&ta, &tb)
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    let inter = a.intersection(b).count() as f32;
    let uni = a.union(b).count() as f32;
    if uni == 0.0 {
        1.0
    } else {
        inter / uni
    }
}

/// Order-aware similarity: longest common subsequence over max length.
fn seq_similarity(a: &[String], b: &[String]) -> f32 {
    let (n, m) = (a.len(), b.len());
    if n == 0 || m == 0 {
        return 0.0;
    }
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in 1..=n {
        for j in 1..=m {
            dp[i][j] = if a[i - 1] == b[j - 1] { dp[i - 1][j - 1] + 1 } else { dp[i - 1][j].max(dp[i][j - 1]) };
        }
    }
    dp[n][m] as f32 / n.max(m) as f32
}

fn ratio(a: usize, b: usize) -> f32 {
    if a == 0 && b == 0 {
        1.0
    } else {
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        lo as f32 / hi.max(1) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aysar_core::capture::{Heading, Link};
    use aysar_core::Mode;

    fn page(path: &str, title: &str, heads: &[(u8, &str)], links: &[&str], words: usize) -> PageCapture {
        PageCapture {
            path: path.into(),
            title: Some(title.into()),
            headings: heads.iter().map(|(l, t)| Heading { level: *l, text: t.to_string() }).collect(),
            links: links.iter().map(|h| Link { text: h.to_string(), href: h.to_string(), internal: true }).collect(),
            landmarks: vec!["nav".into(), "main".into(), "footer".into()],
            word_count: words,
            ..Default::default()
        }
    }

    fn cap(pages: Vec<PageCapture>) -> Capture {
        Capture { target: "t".into(), mode: Mode::CleanRoom, captured_at: String::new(), pages, limitations: vec![] }
    }

    #[test]
    fn identical_scores_100() {
        let a = cap(vec![page("/", "Home", &[(1, "Hi"), (2, "More")], &["/about"], 100)]);
        let r = compare(&a, &a);
        assert!((r.score - 100.0).abs() < 0.01, "{}", r.score);
        assert!(r.mismatches.is_empty());
    }

    #[test]
    fn missing_page_reported() {
        let a = cap(vec![
            page("/", "Home", &[(1, "Hi")], &["/about"], 100),
            page("/about", "About", &[(1, "About")], &["/"], 50),
        ]);
        let b = cap(vec![page("/", "Home", &[(1, "Hi")], &["/about"], 100)]);
        let r = compare(&a, &b);
        assert!(r.mismatches.iter().any(|m| m.contains("missing from the build")));
        assert!(r.score < 60.0);
    }

    #[test]
    fn visual_leg_scores_and_explains() {
        use aysar_core::capture::{LayoutBox, Viewport};
        let vp = |h: u32, hash: &str, header_h: f32| Viewport {
            width: 1280,
            height: h,
            screenshot: None,
            dhash: Some(hash.into()),
            boxes: vec![
                LayoutBox { kind: "header".into(), x: 0.0, y: 0.0, w: 1280.0, h: header_h },
                LayoutBox { kind: "main".into(), x: 0.0, y: header_h, w: 1280.0, h: h as f32 - header_h - 80.0 },
                LayoutBox { kind: "footer".into(), x: 0.0, y: h as f32 - 80.0, w: 1280.0, h: 80.0 },
            ],
        };
        let mut o = page("/", "Home", &[(1, "Hi")], &["/about"], 100);
        o.viewports = vec![vp(1000, "ffff0000ffff0000", 64.0)];
        let mut same = o.clone();
        same.viewports = vec![vp(1000, "ffff0000ffff0000", 64.0)];
        let r = compare(&cap(vec![o.clone()]), &cap(vec![same]));
        assert!(r.visual.unwrap() > 99.0);
        assert!((r.score - 100.0).abs() < 0.1, "{}", r.score);

        let mut diff = o.clone();
        diff.viewports = vec![vp(2600, "0000ffff0000ffff", 300.0)];
        let r = compare(&cap(vec![o.clone()]), &cap(vec![diff]));
        assert!(r.visual.unwrap() < 50.0, "{}", r.visual.unwrap());
        assert!(r.mismatches.iter().any(|m| m.contains("px tall")), "{:?}", r.mismatches);
        assert!(r.mismatches.iter().any(|m| m.contains("looks different")));

        let mut unrendered = o.clone();
        unrendered.viewports = vec![];
        let r = compare(&cap(vec![o]), &cap(vec![unrendered]));
        assert_eq!(r.visual, Some(0.0));
        assert!(r.mismatches.iter().any(|m| m.contains("could not be rendered")));
    }

    #[test]
    fn path_normalisation() {
        assert_eq!(norm_path("about.html"), "/about");
        assert_eq!(norm_path("/about/index.html"), "/about");
        assert_eq!(norm_path("/"), "/");
        assert_eq!(norm_path("./index.html"), "/");
    }
}
