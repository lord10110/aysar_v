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
    /// Colours seen in inline styles / style tags. Deep mode only.
    pub colors: Vec<String>,
    /// Font families seen in inline styles / style tags. Deep mode only.
    pub fonts: Vec<String>,
    /// External stylesheet URLs. Deep mode only.
    pub stylesheets: Vec<String>,
    /// External script URLs. Deep mode only.
    pub scripts: Vec<String>,
    /// SHA-256 of the raw bytes, so a page can be recognised later.
    pub content_hash: String,
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
