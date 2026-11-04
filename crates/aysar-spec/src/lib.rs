//! Spec generation: observation → specification.
//!
//! The generator is deliberately rule-based and deterministic. An AI model may
//! later *enrich* a spec (purposes, behaviours), but the base spec is always
//! reproducible from the capture, so it can be diffed and trusted.

use aysar_core::capture::Capture;
use aysar_core::evidence::{EvidenceKind, Ledger};
use aysar_core::spec::{Behavior, DesignSystem, PageSpec, Region, Rules, Spec, ViewportSpec, SPEC_VERSION};
use aysar_core::Kind;
use std::collections::BTreeSet;

/// Inputs to spec generation.
pub struct SpecInput<'a> {
    pub name: &'a str,
    pub request: &'a str,
    pub kind: Kind,
    pub capture: &'a Capture,
    /// Extra rules from the user.
    pub extra_rules: Vec<String>,
}

/// Build a spec from a capture, recording evidence along the way.
pub fn generate(input: SpecInput<'_>, ledger: Option<&Ledger>) -> anyhow::Result<Spec> {
    let c = input.capture;
    let mut pages = Vec::new();
    let mut colors = BTreeSet::new();
    let mut fonts = BTreeSet::new();
    let mut behaviors = Vec::new();

    for p in &c.pages {
        let text_sample = first_words(&p.text, 60);
        let mut purpose = None;
        if p.path == "/" {
            purpose = Some("Landing page: the first thing a visitor sees.".to_string());
        } else if !p.forms.is_empty() {
            purpose = Some("A page with a form: collects input from the visitor.".to_string());
        }

        let page_ev = ledger
            .map(|l| {
                l.record(
                    EvidenceKind::Observation,
                    format!(
                        "page {} titled {:?} with {} headings, {} links, {} forms",
                        p.path,
                        p.title,
                        p.headings.len(),
                        p.links.len(),
                        p.forms.len()
                    ),
                    &p.source,
                    1.0,
                    vec![],
                )
            })
            .transpose()?;

        for (i, f) in p.forms.iter().enumerate() {
            let id = format!("form-{}-{}", slug(&p.path), i + 1);
            let fields: Vec<String> = f.fields.iter().map(|x| format!("{} ({})", x.name, x.input_type)).collect();
            let desc = format!(
                "On `{}`, a {} form{} with fields: {}. Submitting it{} must validate required fields and give feedback.",
                p.path,
                f.method,
                f.action.as_ref().map(|a| format!(" to `{a}`")).unwrap_or_default(),
                if fields.is_empty() { "(none)".into() } else { fields.join(", ") },
                f.submit_label.as_ref().map(|s| format!(" via \"{s}\"")).unwrap_or_default()
            );
            behaviors.push(Behavior {
                id: id.clone(),
                description: desc.clone(),
                evidence: page_ev.iter().cloned().collect(),
            });
            if let Some(l) = ledger {
                l.record(EvidenceKind::Inference, desc, format!("spec:{id}"), 0.9, page_ev.iter().cloned().collect())?;
            }
        }

        let internal_nav: Vec<_> = p.links.iter().filter(|l| l.internal).cloned().collect();
        if internal_nav.len() > 1 {
            let id = format!("nav-{}", slug(&p.path));
            let targets: Vec<String> = internal_nav.iter().take(12).map(|l| l.href.clone()).collect();
            let desc = format!("Navigation on `{}` reaches: {}.", p.path, targets.join(", "));
            behaviors.push(Behavior { id, description: desc, evidence: page_ev.iter().cloned().collect() });
        }

        for col in &p.colors {
            colors.insert(col.clone());
        }
        for f in &p.fonts {
            fonts.insert(f.clone());
        }

        pages.push(PageSpec {
            path: p.path.clone(),
            title: p.title.clone(),
            purpose,
            headings: p.headings.clone(),
            navigation: p.links.iter().filter(|l| l.internal).cloned().collect(),
            forms: p.forms.clone(),
            landmarks: p.landmarks.clone(),
            text_sample,
            word_count: p.word_count,
            image_count: p.image_count,
            viewports: p
                .viewports
                .iter()
                .map(|v| ViewportSpec {
                    width: v.width,
                    height: v.height,
                    regions: v
                        .boxes
                        .iter()
                        .filter(|b| {
                            matches!(b.kind.as_str(), "header" | "nav" | "main" | "footer" | "h1" | "form" | "aside")
                        })
                        .map(|b| Region {
                            kind: b.kind.clone(),
                            top: b.y.max(0.0) as u32,
                            height: b.h.max(0.0) as u32,
                            width: b.w.max(0.0) as u32,
                        })
                        .collect(),
                })
                .collect(),
        });
    }

    let design = extract_design(c, colors, fonts, ledger)?;

    let stack = detect_stack(input.request);
    let mut limitations = c.limitations.clone();
    if c.pages.is_empty() {
        limitations.push("No pages were captured; the build follows the request alone.".into());
    }

    let spec = Spec {
        spec_version: SPEC_VERSION,
        name: input.name.to_string(),
        kind: input.kind,
        request: input.request.to_string(),
        source: c.target.clone(),
        mode: c.mode.to_string(),
        generated_at: aysar_core::now_rfc3339(),
        stack,
        pages,
        design,
        behaviors,
        rules: Rules { extra: input.extra_rules, ..Default::default() },
        limitations,
    };

    if let Some(l) = ledger {
        l.record(
            EvidenceKind::Action,
            format!("spec generated: {} pages, {} behaviours", spec.pages.len(), spec.behaviors.len()),
            "aysar-spec",
            1.0,
            vec![],
        )?;
    }
    Ok(spec)
}

/// Build the design system: measured styles win; inline-style colours and
/// fonts (Deep mode) are the fallback; otherwise a note tells the builder
/// to choose.
fn extract_design(
    c: &Capture,
    inline_colors: BTreeSet<String>,
    inline_fonts: BTreeSet<String>,
    ledger: Option<&Ledger>,
) -> anyhow::Result<DesignSystem> {
    let mut notes = Vec::new();
    let measured: Vec<&aysar_core::capture::StyleSample> = c.pages.iter().filter_map(|p| p.style.as_ref()).collect();

    let mut d = DesignSystem::default();
    if !measured.is_empty() {
        let mut colors: std::collections::BTreeMap<String, u32> = Default::default();
        let mut fonts: std::collections::BTreeMap<String, u32> = Default::default();
        let mut sizes = Vec::new();
        let mut spacing = Vec::new();
        let mut radii = Vec::new();
        for st in &measured {
            for (col, n) in &st.colors {
                *colors.entry(col.clone()).or_default() += n;
            }
            for (f, n) in &st.fonts {
                *fonts.entry(f.clone()).or_default() += n;
            }
            sizes.extend(st.font_sizes.iter().copied());
            spacing.extend(st.spacing.iter().copied());
            radii.extend(st.radii.iter().copied());
        }
        let mut colors: Vec<(String, u32)> = colors.into_iter().collect();
        colors.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut fonts: Vec<(String, u32)> = fonts.into_iter().collect();
        fonts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        d.colors = colors.into_iter().take(10).map(|(c, _)| c).collect();
        d.fonts = fonts.into_iter().take(5).map(|(f, _)| f).collect();
        d.type_scale = dedup_sorted(sizes, 10);
        d.spacing = dedup_sorted(spacing, 10);
        d.radii = dedup_sorted(radii, 6);
        // Background/foreground from the home page if measured, else the first page.
        let home = c.pages.iter().find(|p| p.path == "/").and_then(|p| p.style.as_ref()).or(measured.first().copied());
        if let Some(h) = home {
            d.background = h.background.clone();
            d.foreground = h.foreground.clone();
        }
        notes.push(format!("Colours, fonts, type scale and spacing were measured on the rendered page at {} width(s); match them closely.", c.pages.iter().map(|p| p.viewports.len()).max().unwrap_or(0)));
        if let Some(l) = ledger {
            l.record(
                EvidenceKind::Inference,
                format!(
                    "design system: {} colours, fonts {:?}, type scale {:?}",
                    d.colors.len(),
                    d.fonts,
                    d.type_scale
                ),
                "render:computed-styles",
                0.9,
                vec![],
            )?;
        }
    } else {
        d.colors = inline_colors.into_iter().collect();
        d.fonts = inline_fonts.into_iter().collect();
        if d.colors.is_empty() && d.fonts.is_empty() {
            notes.push("No colours or fonts were measured (no renderer, and Clean room reads no styles). Choose a clean, accessible palette and system fonts, and keep the layout structure above.".into());
        } else {
            notes
                .push("Colours and fonts come from inline styles only; a renderer would measure the real ones.".into());
        }
    }

    if let Some(home) = c.pages.iter().find(|p| p.path == "/") {
        if home.landmarks.iter().any(|l| l.starts_with("nav")) {
            notes.push("The original has a navigation landmark; keep a visible primary navigation.".into());
        }
        if home.landmarks.iter().any(|l| l.starts_with("footer")) {
            notes.push("The original has a footer; include one.".into());
        }
    }
    d.notes = notes;
    Ok(d)
}

fn dedup_sorted(mut v: Vec<f32>, max: usize) -> Vec<f32> {
    v.retain(|x| x.is_finite());
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut out: Vec<f32> = Vec::new();
    for x in v {
        if out.last().map(|l| (l - x).abs() > 0.5).unwrap_or(true) {
            out.push(x);
        }
    }
    out.truncate(max);
    out
}

/// Guess the requested stack from the request text.
pub fn detect_stack(request: &str) -> Option<String> {
    let r = request.to_ascii_lowercase();
    let known = [
        ("next.js", "Next.js"),
        ("nextjs", "Next.js"),
        ("react", "React"),
        ("vue", "Vue"),
        ("svelte", "Svelte"),
        ("astro", "Astro"),
        ("tailwind", "Tailwind CSS"),
        ("plain html", "Plain HTML/CSS/JS"),
        ("static html", "Plain HTML/CSS/JS"),
        ("vanilla", "Plain HTML/CSS/JS"),
        ("rust", "Rust"),
        ("bevy", "Rust (Bevy)"),
        ("godot", "Godot"),
        ("unity", "Unity"),
        ("unreal", "Unreal Engine"),
        ("python", "Python"),
        ("django", "Python (Django)"),
        ("flask", "Python (Flask)"),
        ("flutter", "Flutter"),
        ("swift", "Swift"),
        ("kotlin", "Kotlin"),
        ("tauri", "Tauri"),
        ("electron", "Electron"),
        ("golang", "Go"),
        ("in go", "Go"),
        ("with go", "Go"),
        ("using go", "Go"),
    ];
    let mut found: Vec<&str> = Vec::new();
    for (needle, label) in known {
        if contains_word(&r, needle.trim()) && !found.contains(&label) {
            found.push(label);
        }
    }
    if found.is_empty() {
        None
    } else {
        Some(found.join(" + "))
    }
}

/// `needle` must appear with non-alphanumeric characters (or the ends) on
/// both sides, so "trust" is not Rust and "swiftly" is not Swift.
fn contains_word(hay: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(i) = hay[from..].find(needle) {
        let start = from + i;
        let end = start + needle.len();
        let before_ok = start == 0 || !hay[..start].chars().next_back().map(|c| c.is_alphanumeric()).unwrap_or(false);
        let after_ok = end == hay.len() || !hay[end..].chars().next().map(|c| c.is_alphanumeric()).unwrap_or(false);
        if before_ok && after_ok {
            return true;
        }
        from = end;
    }
    false
}

fn first_words(s: &str, n: usize) -> String {
    let words: Vec<&str> = s.split_whitespace().take(n).collect();
    let mut out = words.join(" ");
    if s.split_whitespace().count() > n {
        out.push('…');
    }
    out
}

fn slug(path: &str) -> String {
    let s: String = path.chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "home".into()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aysar_core::capture::{Field, Form, Heading, Link, PageCapture};
    use aysar_core::Mode;

    #[test]
    fn generates_pages_and_behaviours() {
        let cap = Capture {
            target: "https://x.test".into(),
            mode: Mode::CleanRoom,
            captured_at: String::new(),
            pages: vec![PageCapture {
                path: "/".into(),
                title: Some("X".into()),
                headings: vec![Heading { level: 1, text: "Hi".into() }],
                links: vec![
                    Link { text: "a".into(), href: "/a".into(), internal: true },
                    Link { text: "b".into(), href: "/b".into(), internal: true },
                ],
                forms: vec![Form {
                    action: Some("/s".into()),
                    method: "POST".into(),
                    fields: vec![Field {
                        name: "email".into(),
                        input_type: "email".into(),
                        placeholder: None,
                        required: true,
                    }],
                    submit_label: Some("Go".into()),
                }],
                text: "hello world".into(),
                word_count: 2,
                landmarks: vec!["nav".into(), "footer".into()],
                ..Default::default()
            }],
            limitations: vec![],
        };
        let spec = generate(
            SpecInput {
                name: "x",
                request: "Recreate this in React with Tailwind",
                kind: Kind::Website,
                capture: &cap,
                extra_rules: vec![],
            },
            None,
        )
        .unwrap();
        assert_eq!(spec.pages.len(), 1);
        assert_eq!(spec.behaviors.len(), 2);
        assert_eq!(spec.stack.as_deref(), Some("React + Tailwind CSS"));
        assert_eq!(detect_stack("a site I can trust, swiftly, go to it"), None);
        assert_eq!(detect_stack("Rebuild in Rust please"), Some("Rust".into()));
        assert!(spec.to_markdown().contains("## Pages"));
        let round: Spec = Spec::from_json(&spec.to_json()).unwrap();
        assert_eq!(round.pages[0].headings[0].text, "Hi");
    }
}
