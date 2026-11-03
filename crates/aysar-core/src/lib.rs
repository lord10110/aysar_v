//! Aysar core: the shared vocabulary every other crate speaks.
//!
//! - [`Mode`]: Clean room (observe only) or Deep (read everything, for private use).
//! - [`Target`]: what we are studying.
//! - [`capture`]: what was observed about the target.
//! - [`spec`]: the intermediate specification (IR) generated from a capture.
//! - [`evidence`]: a ledger linking every claim to its source and confidence.
//! - [`project`]: the on-disk project layout and its state file.

pub mod capture;
pub mod evidence;
pub mod project;
pub mod spec;

use serde::{Deserialize, Serialize};
use std::fmt;

/// How much of the target Aysar is allowed to look at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Only what a visitor can see: visible text, structure, measured layout.
    /// Source, scripts and binaries are never read.
    #[default]
    CleanRoom,
    /// Everything: source, scripts, files, binaries. For your private use.
    Deep,
}

impl Mode {
    pub fn is_deep(self) -> bool {
        matches!(self, Mode::Deep)
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mode::CleanRoom => write!(f, "clean-room"),
            Mode::Deep => write!(f, "deep"),
        }
    }
}

/// The kind of thing we are recreating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Website,
    App,
    Game,
    Unknown,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Kind::Website => "website",
            Kind::App => "app",
            Kind::Game => "game",
            Kind::Unknown => "unknown",
        };
        write!(f, "{s}")
    }
}

/// What Aysar points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Target {
    /// A website reachable over HTTP(S).
    Website { url: String },
    /// A local file or folder (an app binary, a game folder, an HTML export).
    Local { path: String },
    /// No target: build from the request alone.
    None,
}

impl Target {
    /// Parse a user-supplied string: `-` means no target, `http(s)://` is a
    /// website, anything else is a local path.
    pub fn parse(s: &str) -> Target {
        let s = s.trim();
        if s.is_empty() || s == "-" {
            Target::None
        } else if s.starts_with("http://") || s.starts_with("https://") {
            Target::Website { url: s.to_string() }
        } else {
            Target::Local { path: s.to_string() }
        }
    }

    pub fn kind_hint(&self) -> Kind {
        match self {
            Target::Website { .. } => Kind::Website,
            Target::Local { path } => {
                let p = path.to_ascii_lowercase();
                if p.ends_with(".html") || p.ends_with(".htm") {
                    Kind::Website
                } else if p.ends_with(".exe") || p.ends_with(".app") || p.ends_with(".apk") || p.ends_with(".dmg") {
                    Kind::App
                } else {
                    Kind::Unknown
                }
            }
            Target::None => Kind::Unknown,
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Website { url } => write!(f, "{url}"),
            Target::Local { path } => write!(f, "{path}"),
            Target::None => write!(f, "-"),
        }
    }
}

/// A SHA-256 hex digest of some bytes. Used to fingerprint captured content.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Current UTC timestamp in RFC 3339.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_parse() {
        assert_eq!(Target::parse("-"), Target::None);
        assert_eq!(Target::parse("https://example.com"), Target::Website { url: "https://example.com".into() });
        assert_eq!(Target::parse("./site/index.html"), Target::Local { path: "./site/index.html".into() });
        assert_eq!(Target::parse("./site/index.html").kind_hint(), Kind::Website);
    }

    #[test]
    fn sha() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }
}
