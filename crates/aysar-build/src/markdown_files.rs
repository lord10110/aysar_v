//! Fallback for the built-in builder: files given as Markdown fences.
//!
//! Small models often ignore the `<<<FILE …>>>` protocol and answer the way
//! chat assistants usually do — a heading or a sentence naming the file
//! (`### index.html`), then a fenced block (three backticks, `html`, the
//! file, three backticks).
//!
//! This module turns such a reply into `(path, body)` pairs. The name comes
//! from, in order: the fence's info string (` ```html:index.html `,
//! ` ```css title="site.css" `), the nearest preceding line that names a
//! file, or a default for the language (`index.html`, `styles.css`,
//! `script.js`, …). Defaults are then renamed to whatever the HTML actually
//! links, so `<link href="main.css">` and a nameless CSS fence line up.

const EXTENSIONS: &[&str] = &[
    "html",
    "htm",
    "css",
    "js",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "jsx",
    "json",
    "md",
    "txt",
    "svg",
    "xml",
    "toml",
    "yml",
    "yaml",
    "py",
    "rs",
    "scss",
    "vue",
    "astro",
    "env",
    "ini",
    "cfg",
    "sh",
    "csv",
    "webmanifest",
];

/// Parse Markdown-fenced files. Returns an empty vec when nothing looks like a file.
pub fn parse(reply: &str) -> Vec<(String, String)> {
    let mut files: Vec<(String, String, bool)> = Vec::new(); // (path, body, name_was_guessed)
    let mut pending_name: Option<String> = None;
    let mut since_name = 0usize;
    let mut open: Option<(String, bool, String)> = None; // (path, guessed, body)
    let mut defaults_used = std::collections::HashMap::<String, usize>::new();

    for line in reply.lines() {
        let trimmed = line.trim();
        if let Some((path, guessed, body)) = open.as_mut() {
            if trimmed.starts_with("```") && trimmed.trim_start_matches('`').trim().is_empty() {
                let (p, g, b) = (path.clone(), *guessed, body.clone());
                open = None;
                if !b.trim().is_empty() {
                    files.push((p, b, g));
                }
                pending_name = None;
            } else {
                body.push_str(line);
                body.push('\n');
            }
            continue;
        }
        if let Some(info) = trimmed.strip_prefix("```") {
            let info = info.trim();
            let lang = info.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("").to_ascii_lowercase();
            let named = name_in(info).or_else(|| if since_name <= 3 { pending_name.take() } else { None });
            let (path, guessed) = match named {
                Some(n) => (n, false),
                None => {
                    let base = default_name(&lang);
                    let count = defaults_used.entry(base.to_string()).or_insert(0);
                    *count += 1;
                    let name = if *count == 1 {
                        base.to_string()
                    } else {
                        let (stem, ext) = base.rsplit_once('.').unwrap_or((base, ""));
                        format!("{stem}-{count}.{ext}")
                    };
                    (name, true)
                }
            };
            open = Some((path, guessed, String::new()));
            continue;
        }
        if let Some(n) = name_in(trimmed) {
            pending_name = Some(n);
            since_name = 0;
        } else if !trimmed.is_empty() {
            since_name += 1;
        }
    }
    if let Some((path, guessed, body)) = open.take() {
        if !body.trim().is_empty() {
            files.push((path, body, guessed));
        }
    }

    rename_guessed_to_linked(&mut files);
    files.into_iter().map(|(p, b, _)| (p, b)).collect()
}

/// A file name mentioned in a line, if the line is mostly that name
/// (`### index.html`, `**styles.css**`, `File: app.js`, `<!-- index.html -->`, `html:index.html`).
fn name_in(line: &str) -> Option<String> {
    let cleaned = line
        .trim()
        .trim_start_matches(|c: char| "#*->`_".contains(c) || c.is_whitespace())
        .trim_start_matches("<!--")
        .trim_start_matches("/*")
        .trim_start_matches("//")
        .trim_end_matches("-->")
        .trim_end_matches("*/")
        .trim_matches(|c: char| "*`_:)".contains(c) || c.is_whitespace());
    let cleaned = ["File:", "file:", "Filename:", "filename:", "Path:", "path:", "title=", "name=", "Create", "create"]
        .iter()
        .fold(cleaned, |s, p| s.strip_prefix(p).map(str::trim).unwrap_or(s));
    // A fence info string may be `html:index.html`, `html index.html` or `html title="x.css"`.
    for token in cleaned.split(|c: char| c.is_whitespace() || c == ':' || c == '"' || c == '\'' || c == '=') {
        let t = token.trim_matches(|c: char| "`*_(),;".contains(c));
        if looks_like_path(t) {
            return Some(t.to_string());
        }
    }
    None
}

fn looks_like_path(t: &str) -> bool {
    let Some((stem, ext)) = t.rsplit_once('.') else { return false };
    !stem.is_empty()
        && !stem.ends_with('/')
        && EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
        && t.len() < 200
        && t.chars().all(|c| c.is_alphanumeric() || "./-_@".contains(c))
        && crate::api::safe_path(t)
        && !t.starts_with("./")
        && !t.starts_with("www.")
        && !t.starts_with("http")
}

fn default_name(lang: &str) -> &'static str {
    match lang {
        "html" | "htm" | "xhtml" => "index.html",
        "css" => "styles.css",
        "js" | "javascript" => "script.js",
        "ts" | "typescript" => "script.ts",
        "json" => "data.json",
        "md" | "markdown" => "README.md",
        "svg" => "image.svg",
        "txt" | "text" | "" => "notes.txt",
        _ => "file.txt",
    }
}

/// `styles.css` guessed from a nameless CSS fence becomes `main.css` when the
/// HTML links `main.css` and nothing else of that kind.
fn rename_guessed_to_linked(files: &mut [(String, String, bool)]) {
    let html: String = files.iter().filter(|(p, _, _)| p.ends_with(".html")).map(|(_, b, _)| b.as_str()).collect();
    let linked_css = attr_values(&html, "href=", ".css");
    let linked_js = attr_values(&html, "src=", ".js");
    for (ext, linked) in [(".css", linked_css), (".js", linked_js)] {
        let guessed: Vec<usize> =
            files.iter().enumerate().filter(|(_, (p, _, g))| *g && p.ends_with(ext)).map(|(i, _)| i).collect();
        if guessed.len() == 1 && linked.len() == 1 {
            let target = &linked[0];
            if !files.iter().any(|(p, _, _)| p == target) {
                files[guessed[0]].0 = target.clone();
            }
        }
    }
}

fn attr_values(html: &str, attr: &str, ext: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, _) in html.match_indices(attr) {
        let rest = &html[i + attr.len()..];
        let rest = rest.trim_start_matches(['"', '\'']);
        let end = rest.find(['"', '\'', ' ', '>']).unwrap_or(rest.len());
        let v = rest[..end].trim_start_matches("./");
        if v.ends_with(ext) && !v.contains("//") && crate::api::safe_path(v) && !out.contains(&v.to_string()) {
            out.push(v.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_name_the_fences() {
        let reply = "Sure! Here is the site.\n\n### index.html\n```html\n<!doctype html><link rel=\"stylesheet\" href=\"styles.css\"><script src=\"app.js\"></script>\n```\n\n**styles.css**\n```css\nbody{margin:0}\n```\n\nFile: `app.js`\n```javascript\nconsole.log(1)\n```\nThat's all.\n";
        let files = parse(reply);
        let names: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(names, vec!["index.html", "styles.css", "app.js"]);
        assert_eq!(files[1].1, "body{margin:0}\n");
    }

    #[test]
    fn nameless_fences_get_defaults_then_linked_names() {
        let reply = "```html\n<html><head><link href=\"css/site.css\" rel=\"stylesheet\"></head><body><script src=\"main.js\"></script></body></html>\n```\n```css\nh1{color:red}\n```\n```js\nalert(1)\n```\n```html\n<p>about</p>\n```\n";
        let files = parse(reply);
        let names: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(names, vec!["index.html", "css/site.css", "main.js", "index-2.html"]);
    }

    #[test]
    fn info_string_names_win() {
        let reply = "```html:pages/learn.html\n<h1>Learn</h1>\n```\n```css title=\"theme.css\"\n:root{}\n```\n";
        let files = parse(reply);
        let names: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(names, vec!["pages/learn.html", "theme.css"]);
    }

    #[test]
    fn prose_without_fences_is_nothing() {
        assert!(parse("I cannot do that. Visit index.html for details.").is_empty());
        assert!(parse("").is_empty());
    }

    #[test]
    fn unsafe_names_are_not_used() {
        let reply = "### ../../etc/passwd.txt\n```\nroot\n```\n";
        let files = parse(reply);
        assert_eq!(files[0].0, "notes.txt");
    }
}
