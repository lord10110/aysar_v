//! What was observed about a target. A capture is raw observation; the
//! [`crate::spec`] is the interpretation built from it.

use crate::Mode;
use serde::{Deserialize, Serialize};

/// A heading on a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heading {
    pub level: u8,
    pub text: String,
}

/// A link on a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub text: String,
    pub href: String,
    /// True when the link stays on the same origin as the page.
    pub internal: bool,
}

/// A field inside a form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub input_type: String,
    pub placeholder: Option<String>,
    pub required: bool,
}

/// A form on a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Form {
    pub action: Option<String>,
    pub method: String,
    pub fields: Vec<Field>,
    pub submit_label: Option<String>,
}

/// Everything observed on one page.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageCapture {
    /// Where the page came from (URL or local path).
    pub source: String,
    /// Path relative to the site root, e.g. `/` or `/about`.
    pub path: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub lang: Option<String>,
    pub headings: Vec<Heading>,
    pub links: Vec<Link>,
    pub forms: Vec<Form>,
    /// Visible text, whitespace-normalised, truncated to a sensible size.
    pub text: String,
    pub word_count: usize,
    pub image_count: usize,
    /// Semantic landmarks found (`header`, `nav`, `main`, `footer`, ...).
    pub landmarks: Vec<String>,
    /// Colours: from inline styles / style tags in Deep mode, or measured
    /// on the rendered page (any mode, since a visitor can see them).
    pub colors: Vec<String>,
    /// Font families, same sources as `colors`.
    pub fonts: Vec<String>,
    /// External stylesheet URLs. Deep mode only.
    pub stylesheets: Vec<String>,
    /// External script URLs. Deep mode only.
    pub scripts: Vec<String>,
    /// SHA-256 of the raw bytes, so a page can be recognised later.
    pub content_hash: String,
    /// What the page looks like at each width, when a renderer was available.
    #[serde(default)]
    pub viewports: Vec<Viewport>,
    /// Measured styles from the rendered page, when a renderer was available.
    #[serde(default)]
    pub style: Option<StyleSample>,
    /// Links and forms exercised on the rendered page, when a renderer was available.
    #[serde(default)]
    pub behavior: Option<BehaviorProbe>,
}

/// A rectangle on the rendered page, in CSS pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutBox {
    /// What it is: `header`, `nav`, `main`, `footer`, `h1`, `form`, `img`, …
    pub kind: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl LayoutBox {
    pub fn area(&self) -> f32 {
        self.w.max(0.0) * self.h.max(0.0)
    }

    /// Intersection over union with another box.
    pub fn iou(&self, o: &LayoutBox) -> f32 {
        let x1 = self.x.max(o.x);
        let y1 = self.y.max(o.y);
        let x2 = (self.x + self.w).min(o.x + o.w);
        let y2 = (self.y + self.h).min(o.y + o.h);
        let inter = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
        let union = self.area() + o.area() - inter;
        if union <= 0.0 {
            0.0
        } else {
            inter / union
        }
    }
}

/// The page rendered at one width.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Viewport {
    pub width: u32,
    /// Full document height at this width.
    pub height: u32,
    /// Path of the full-page PNG, relative to the capture folder.
    pub screenshot: Option<String>,
    /// Difference hash of the screenshot (16 hex chars = 64 bits), for
    /// cheap visual similarity.
    pub dhash: Option<String>,
    pub boxes: Vec<LayoutBox>,
}

/// Styles measured on the rendered page.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StyleSample {
    /// Colour → how many elements use it (text or background).
    pub colors: Vec<(String, u32)>,
    /// Font family → how many elements use it.
    pub fonts: Vec<(String, u32)>,
    /// Distinct font sizes in px, ascending.
    pub font_sizes: Vec<f32>,
    /// Distinct margin/padding values in px, ascending.
    pub spacing: Vec<f32>,
    /// Distinct border radii in px, ascending.
    pub radii: Vec<f32>,
    /// Body background colour.
    pub background: Option<String>,
    /// Body text colour.
    pub foreground: Option<String>,
}

/// A whole capture of a target.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Capture {
    pub target: String,
    pub mode: Mode,
    pub captured_at: String,
    pub pages: Vec<PageCapture>,
    /// Anything the capture could not do, in plain words.
    pub limitations: Vec<String>,
}

impl Capture {
    pub fn page(&self, path: &str) -> Option<&PageCapture> {
        self.pages.iter().find(|p| p.path == path)
    }

    pub fn total_words(&self) -> usize {
        self.pages.iter().map(|p| p.word_count).sum()
    }
}

/// What happened when the rendered page was *driven*: links resolved,
/// forms validated. Measured on both the original and the build.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BehaviorProbe {
    /// Internal link href → did it resolve (HTTP 2xx/3xx on the same origin)?
    pub links: Vec<(String, bool)>,
    pub forms: Vec<FormProbe>,
    /// Elements whose style changes on hover, e.g. `nav a`, `button`.
    pub hover_changes: Vec<String>,
    /// Elements that show a focus style when focused.
    pub focus_changes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FormProbe {
    pub required_fields: usize,
    /// `form.checkValidity()` is false when required fields are empty.
    pub blocks_empty_submit: bool,
    /// `form.checkValidity()` is true once every field holds a plausible value.
    pub accepts_filled: bool,
    pub has_submit: bool,
}
