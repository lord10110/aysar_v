//! Verification: compare what was built with what was observed.
//!
//! The score is composite. Today it has two legs, **structure** (pages,
//! headings, landmarks, navigation, forms) and **content shape** (word
//! counts, image counts). Two more legs are planned and have reserved
//! weights: **visual** (rendered screenshots at three widths) and
//! **behaviour** (driving both sites and comparing state machines).
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
    /// Reserved: `None` until a renderer is wired in.
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
        s.push_str("\n\n## Pages\n\n| Page | Score | Title | Headings | Landmarks | Nav | Forms | Content |\n|---|---|---|---|---|---|---|---|\n");
        for p in &self.pages {
            if p.found {
                s.push_str(&format!(
                    "| `{}` | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} |\n",
                    p.path,
                    p.score,
                    p.title_match * 100.0,
                    p.headings * 100.0,
                    p.landmarks * 100.0,
                    p.navigation * 100.0,
                    p.forms * 100.0,
                    p.content_shape * 100.0
                ));
            } else {
                s.push_str(&format!("| `{}` | 0 | missing | | | | | |\n", p.path));
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
    let score = if original.pages.is_empty() {
        // Nothing to compare with: report what exists.
        if build.pages.is_empty() {
            0.0
        } else {
            50.0
        }
    } else {
        structure * 0.7 + content * 0.3
    };

    let mut limitations = vec![
        "Visual similarity (rendered screenshots at 360/768/1280px) is not checked yet.".into(),
        "Behaviour (clicking, submitting, navigating) is not checked yet.".into(),
    ];
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
        visual: None,
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

    let oh: Vec<String> = o.headings.iter().map(|h| format!("h{}:{}", h.level, norm(&h.text))).collect();
    let bh: Vec<String> = b.headings.iter().map(|h| format!("h{}:{}", h.level, norm(&h.text))).collect();
    let headings = if oh.is_empty() && bh.is_empty() { 1.0 } else { seq_similarity(&oh, &bh) };
    if headings < 0.6 {
        let o_list: Vec<String> = o.headings.iter().take(6).map(|h| format!("h{} \"{}\"", h.level, h.text)).collect();
        mm.push(format!(
            "`{}`: heading structure differs; the original has {} headings ({}{}) and the build has {}.",
            o.path,
            o.headings.len(),
            o_list.join(", "),
            if o.headings.len() > 6 { ", …" } else { "" },
            b.headings.len()
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

    let score = ((title_match + headings + landmarks + navigation + forms) / 5.0 * 0.7 + content_shape * 0.3) * 100.0;
    PageReport {
        path: o.path.clone(),
        found: true,
        title_match,
        headings,
        landmarks,
        navigation,
        forms,
        content_shape,
        score,
        mismatches: mm,
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
    fn path_normalisation() {
        assert_eq!(norm_path("about.html"), "/about");
        assert_eq!(norm_path("/about/index.html"), "/about");
        assert_eq!(norm_path("/"), "/");
        assert_eq!(norm_path("./index.html"), "/");
    }
}
