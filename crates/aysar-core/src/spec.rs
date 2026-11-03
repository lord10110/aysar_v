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
}

/// Design tokens inferred from the target.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DesignSystem {
    pub colors: Vec<String>,
    pub fonts: Vec<String>,
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

        if !self.design.colors.is_empty() || !self.design.fonts.is_empty() {
            out.push_str("\n## Design\n\n");
            if !self.design.colors.is_empty() {
                out.push_str(&format!("- Colours: {}\n", self.design.colors.join(", ")));
            }
            if !self.design.fonts.is_empty() {
                out.push_str(&format!("- Fonts: {}\n", self.design.fonts.join(", ")));
            }
            for n in &self.design.notes {
                out.push_str(&format!("- {n}\n"));
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
