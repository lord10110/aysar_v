//! The intermediate specification (IR).
//!
//! Study produces a [`Spec`]; Build consumes it. The spec is the contract
//! between observation and construction: readable, editable, and
//! independent of which coding agent does the building.

use crate::capture::{Form, Heading, Link};
use crate::Kind;
use serde::{Deserialize, Serialize};

pub const SPEC_VERSION: u32 = 1;

/// A page (or screen, or scene) to build.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageSpec {
    pub path: String,
    pub title: Option<String>,
    pub purpose: Option<String>,
    pub headings: Vec<Heading>,
    pub navigation: Vec<Link>,
    pub forms: Vec<Form>,
    pub landmarks: Vec<String>,
    /// A short sample of the visible text, for tone and structure only.
    pub text_sample: String,
    pub word_count: usize,
    pub image_count: usize,
    /// Layout at each measured width, when a renderer was available.
    #[serde(default)]
    pub viewports: Vec<ViewportSpec>,
}

/// How the page is laid out at one width: document height and where the
/// main regions sit. Enough for a builder to match proportions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ViewportSpec {
    pub width: u32,
    pub height: u32,
    /// `kind` → (top, height) for header/nav/main/footer/h1/form…
    pub regions: Vec<Region>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Region {
    pub kind: String,
    pub top: u32,
    pub height: u32,
    pub width: u32,
}

/// Design tokens inferred from the target.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DesignSystem {
    /// Most-used colours first (hex), when measured; else as seen in styles.
    pub colors: Vec<String>,
    pub fonts: Vec<String>,
    /// Distinct font sizes in px, ascending (measured).
    #[serde(default)]
    pub type_scale: Vec<f32>,
    /// Common margin/padding values in px, ascending (measured).
    #[serde(default)]
    pub spacing: Vec<f32>,
    /// Border radii in px, ascending (measured).
    #[serde(default)]
    pub radii: Vec<f32>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub foreground: Option<String>,
    pub notes: Vec<String>,
}

/// A behaviour the build must reproduce. Verification turns these into checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Behavior {
    pub id: String,
    pub description: String,
    /// Where the behaviour was observed.
    pub evidence: Vec<String>,
}

/// Rules the builder must obey. These are always included.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rules {
    /// Never copy logos, photos, or body text from the original.
    pub no_asset_copying: bool,
    /// Never bypass logins, DRM, or anti-cheat.
    pub no_circumvention: bool,
    /// Free-form extra rules from the user.
    pub extra: Vec<String>,
}

impl Default for Rules {
    fn default() -> Self {
        Rules { no_asset_copying: true, no_circumvention: true, extra: vec![] }
    }
}

/// The full specification handed to the builder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spec {
    pub spec_version: u32,
    pub name: String,
    pub kind: Kind,
    /// What the user asked for, verbatim.
    pub request: String,
    /// Where the target was.
    pub source: String,
    /// Which mode produced this spec.
    pub mode: String,
    pub generated_at: String,
    /// Preferred stack (language / framework / engine), if the request names one.
    pub stack: Option<String>,
    pub pages: Vec<PageSpec>,
    pub design: DesignSystem,
    pub behaviors: Vec<Behavior>,
    pub rules: Rules,
    /// For programs and app packages: what the analysis engine found.
    #[serde(default)]
    pub program: Option<crate::capture::ProgramStudy>,
    /// What the study could not learn.
    pub limitations: Vec<String>,
}

impl Spec {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("spec serialises")
    }

    pub fn from_json(s: &str) -> anyhow::Result<Spec> {
        Ok(serde_json::from_str(s)?)
    }

    /// A compact Markdown rendering for prompts and humans.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# Spec: {}\n\n", self.name));
        out.push_str(&format!("- Kind: {}\n- Source: {}\n- Mode: {}\n", self.kind, self.source, self.mode));
        if let Some(stack) = &self.stack {
            out.push_str(&format!("- Stack: {stack}\n"));
        }
        out.push_str(&format!("\n## Request\n\n{}\n", self.request));

        let d = &self.design;
        if !d.colors.is_empty() || !d.fonts.is_empty() || !d.notes.is_empty() {
            out.push_str("\n## Design\n\n");
            if let (Some(bg), Some(fg)) = (&d.background, &d.foreground) {
                out.push_str(&format!("- Page background {bg}, text {fg}\n"));
            }
            if !d.colors.is_empty() {
                out.push_str(&format!("- Colours (most used first): {}\n", d.colors.join(", ")));
            }
            if !d.fonts.is_empty() {
                out.push_str(&format!("- Fonts: {}\n", d.fonts.join(", ")));
            }
            if !d.type_scale.is_empty() {
                out.push_str(&format!("- Type scale (px): {}\n", px_list(&d.type_scale)));
            }
            if !d.spacing.is_empty() {
                out.push_str(&format!("- Spacing values (px): {}\n", px_list(&d.spacing)));
            }
            if !d.radii.is_empty() {
                out.push_str(&format!("- Border radii (px): {}\n", px_list(&d.radii)));
            }
            for n in &d.notes {
                out.push_str(&format!("- {n}\n"));
            }
        }

        if let Some(pr) = &self.program {
            out.push_str("\n## Program (studied with the analysis engine)\n\n");
            out.push_str(&format!("- What it is: {}\n", pr.what));
            if let (Some(f), Some(a)) = (&pr.format, &pr.arch) {
                out.push_str(&format!(
                    "- Format {f}, architecture {a}{}\n",
                    pr.platform.as_ref().map(|p| format!(", {p}")).unwrap_or_default()
                ));
            }
            if let Some(n) = pr.size_bytes {
                out.push_str(&format!("- Size: {} KB\n", n / 1024));
            }
            if !pr.dependencies.is_empty() {
                out.push_str(&format!("- Depends on: {}\n", pr.dependencies.join(", ")));
            }
            if let Some(n) = pr.function_count {
                out.push_str(&format!("- About {n} functions and {} strings\n", pr.string_count.unwrap_or(0)));
            }
            if !pr.imports.is_empty() {
                out.push_str(&format!(
                    "- Uses (imports): {}\n",
                    pr.imports.iter().take(40).cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            if !pr.components.is_empty() {
                out.push_str(&format!(
                    "- Components: {}\n",
                    pr.components.iter().take(30).cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            if !pr.feature_names.is_empty() {
                out.push_str(&format!(
                    "- Names that reveal features: {}\n",
                    pr.feature_names.iter().take(40).cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            if !pr.resources.is_empty() {
                out.push_str(&format!(
                    "- Resources: {}\n",
                    pr.resources.iter().take(30).cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            if !pr.ui_strings.is_empty() {
                out.push_str("- Text the program shows (for the *kind* of screens and messages, not to copy):\n");
                for t in pr.ui_strings.iter().take(40) {
                    out.push_str(&format!("  - {t}\n"));
                }
            }
            if !pr.tools_run.is_empty() {
                out.push_str(&format!("- Engine tools consulted: {}\n", pr.tools_run.join(", ")));
            }
        }

        if !self.pages.is_empty() {
            out.push_str("\n## Pages\n");
            for p in &self.pages {
                out.push_str(&format!("\n### `{}`", p.path));
                if let Some(t) = &p.title {
                    out.push_str(&format!(" — {t}"));
                }
                out.push('\n');
                if !p.landmarks.is_empty() {
                    out.push_str(&format!("- Landmarks: {}\n", p.landmarks.join(", ")));
                }
                out.push_str(&format!("- ~{} words, {} images\n", p.word_count, p.image_count));
                if !p.viewports.is_empty() {
                    out.push_str("- Layout (measured):\n");
                    for v in &p.viewports {
                        let regions: Vec<String> = v
                            .regions
                            .iter()
                            .map(|r| format!("{} at {}px, {}px tall", r.kind, r.top, r.height))
                            .collect();
                        out.push_str(&format!(
                            "  - {}px wide: page is {}px tall; {}\n",
                            v.width,
                            v.height,
                            if regions.is_empty() { "no regions measured".to_string() } else { regions.join("; ") }
                        ));
                    }
                }
                if !p.headings.is_empty() {
                    out.push_str("- Headings:\n");
                    for h in &p.headings {
                        out.push_str(&format!("  - h{}: {}\n", h.level, h.text));
                    }
                }
                if !p.navigation.is_empty() {
                    out.push_str("- Navigation:\n");
                    for l in p.navigation.iter().take(30) {
                        out.push_str(&format!("  - [{}]({})\n", l.text, l.href));
                    }
                }
                if !p.forms.is_empty() {
                    out.push_str("- Forms:\n");
                    for f in &p.forms {
                        let fields: Vec<String> =
                            f.fields.iter().map(|x| format!("{}:{}", x.name, x.input_type)).collect();
                        out.push_str(&format!(
                            "  - {} {} [{}]{}\n",
                            f.method,
                            f.action.clone().unwrap_or_else(|| "(none)".into()),
                            fields.join(", "),
                            f.submit_label.as_ref().map(|s| format!(" submit=\"{s}\"")).unwrap_or_default()
                        ));
                    }
                }
                if !p.text_sample.is_empty() {
                    out.push_str(&format!("- Text sample: {}\n", p.text_sample));
                }
            }
        }

        if !self.behaviors.is_empty() {
            out.push_str("\n## Behaviours\n\n");
            for b in &self.behaviors {
                out.push_str(&format!("- **{}**: {}\n", b.id, b.description));
            }
        }

        out.push_str("\n## Rules\n\n");
        if self.rules.no_asset_copying {
            out.push_str("- Do not copy logos, photos, icons or body text from the original. Write original copy that serves the same purpose.\n");
        }
        if self.rules.no_circumvention {
            out.push_str("- Do not bypass logins, DRM, rate limits or anti-cheat.\n");
        }
        for r in &self.rules.extra {
            out.push_str(&format!("- {r}\n"));
        }

        if !self.limitations.is_empty() {
            out.push_str("\n## Limitations of this study\n\n");
            for l in &self.limitations {
                out.push_str(&format!("- {l}\n"));
            }
        }
        out
    }
}

fn px_list(v: &[f32]) -> String {
    v.iter()
        .map(|x| if (x - x.round()).abs() < 0.05 { format!("{}", x.round() as i64) } else { format!("{x:.1}") })
        .collect::<Vec<_>>()
        .join(", ")
}
