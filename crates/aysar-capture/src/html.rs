//! HTML observation. One function, [`parse_page`], turns a document into a
//! [`PageCapture`], reading more or less depending on [`Mode`].

use aysar_core::capture::{Field, Form, Heading, Link, PageCapture};
use aysar_core::Mode;
use scraper::{ElementRef, Html, Selector};
use std::collections::BTreeSet;
use url::Url;

const MAX_TEXT_CHARS: usize = 4000;

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static selector")
}

fn clean(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn el_text(el: ElementRef<'_>) -> String {
    clean(&el.text().collect::<Vec<_>>().join(" "))
}

/// Parse one HTML document.
///
/// `source` is where it came from, `path` the site-relative path, `base`
/// the URL to resolve links against (none for local files).
pub fn parse_page(body: &str, source: &str, path: &str, base: &Option<Url>, mode: Mode) -> PageCapture {
    let doc = Html::parse_document(body);
    let mut page = PageCapture {
        source: source.to_string(),
        path: path.to_string(),
        content_hash: aysar_core::sha256_hex(body.as_bytes()),
        ..Default::default()
    };

    page.title = doc.select(&sel("title")).next().map(el_text).filter(|t| !t.is_empty());
    page.lang = doc.select(&sel("html")).next().and_then(|h| h.value().attr("lang").map(|s| s.to_string()));
    page.description =
        doc.select(&sel("meta[name=description]")).next().and_then(|m| m.value().attr("content").map(clean));

    for level in 1..=6u8 {
        for h in doc.select(&sel(&format!("h{level}"))) {
            let text = el_text(h);
            if !text.is_empty() {
                page.headings.push(Heading { level, text });
            }
        }
    }
    // Keep document order rather than level order.
    {
        let mut ordered: Vec<Heading> = Vec::new();
        for h in doc.select(&sel("h1, h2, h3, h4, h5, h6")) {
            let level = h.value().name().trim_start_matches('h').parse::<u8>().unwrap_or(6);
            let text = el_text(h);
            if !text.is_empty() {
                ordered.push(Heading { level, text });
            }
        }
        if !ordered.is_empty() {
            page.headings = ordered;
        }
    }

    let origin = base.as_ref().map(|b| b.origin());
    let mut seen_links = BTreeSet::new();
    for a in doc.select(&sel("a[href]")) {
        let href = a.value().attr("href").unwrap_or("").trim().to_string();
        if href.is_empty() || href.starts_with('#') || href.starts_with("javascript:") || href.starts_with("mailto:") {
            continue;
        }
        let text = el_text(a);
        let internal = match (&origin, base) {
            (Some(o), Some(b)) => b.join(&href).map(|u| &u.origin() == o).unwrap_or(false),
            _ => !(href.starts_with("http://") || href.starts_with("https://") || href.starts_with("//")),
        };
        let key = format!("{text}|{href}");
        if seen_links.insert(key) {
            page.links.push(Link { text, href, internal });
        }
    }

    for f in doc.select(&sel("form")) {
        let action = f.value().attr("action").map(|s| s.to_string());
        let method = f.value().attr("method").unwrap_or("get").to_ascii_uppercase();
        let mut fields = Vec::new();
        for i in f.select(&sel("input, textarea, select")) {
            let v = i.value();
            let input_type = match v.name() {
                "input" => v.attr("type").unwrap_or("text").to_string(),
                other => other.to_string(),
            };
            if matches!(input_type.as_str(), "hidden" | "submit" | "button" | "reset") {
                continue;
            }
            fields.push(Field {
                name: v.attr("name").or(v.attr("id")).unwrap_or("").to_string(),
                input_type,
                placeholder: v.attr("placeholder").map(|s| s.to_string()),
                required: v.attr("required").is_some(),
            });
        }
        let submit_label = f
            .select(&sel("button[type=submit], input[type=submit], button"))
            .next()
            .map(|b| {
                let t = el_text(b);
                if t.is_empty() {
                    b.value().attr("value").unwrap_or("").to_string()
                } else {
                    t
                }
            })
            .filter(|s| !s.is_empty());
        page.forms.push(Form { action, method, fields, submit_label });
    }

    page.image_count = doc.select(&sel("img, picture, svg")).count();

    for lm in ["header", "nav", "main", "aside", "footer", "article", "section"] {
        let n = doc.select(&sel(lm)).count();
        if n > 0 {
            page.landmarks.push(if n == 1 { lm.to_string() } else { format!("{lm}×{n}") });
        }
    }

    // Visible text: body minus script/style/noscript/template.
    let mut text = String::new();
    if let Some(body_el) = doc.select(&sel("body")).next() {
        collect_visible_text(body_el, &mut text);
    }
    let text = clean(&text);
    page.word_count = text.split_whitespace().count();
    page.text = if text.len() > MAX_TEXT_CHARS {
        let mut cut = MAX_TEXT_CHARS;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &text[..cut])
    } else {
        text
    };

    if mode.is_deep() {
        for l in doc.select(&sel("link[rel=stylesheet][href]")) {
            if let Some(h) = l.value().attr("href") {
                page.stylesheets.push(h.to_string());
            }
        }
        for s in doc.select(&sel("script[src]")) {
            if let Some(src) = s.value().attr("src") {
                page.scripts.push(src.to_string());
            }
        }
        let mut css = String::new();
        for st in doc.select(&sel("style")) {
            css.push_str(&st.text().collect::<String>());
            css.push('\n');
        }
        for el in doc.select(&sel("[style]")) {
            if let Some(s) = el.value().attr("style") {
                css.push_str(s);
                css.push('\n');
            }
        }
        page.colors = extract_colors(&css);
        page.fonts = extract_fonts(&css);
    }

    page
}

fn collect_visible_text(el: ElementRef<'_>, out: &mut String) {
    for node in el.children() {
        if let Some(child) = ElementRef::wrap(node) {
            let name = child.value().name();
            if matches!(name, "script" | "style" | "noscript" | "template" | "svg") {
                continue;
            }
            collect_visible_text(child, out);
            if matches!(
                name,
                "p" | "div" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "br" | "tr" | "section" | "article"
            ) {
                out.push(' ');
            }
        } else if let Some(t) = node.value().as_text() {
            out.push_str(t);
            out.push(' ');
        }
    }
}

/// Pull hex / rgb colour literals out of CSS text.
pub fn extract_colors(css: &str) -> Vec<String> {
    let mut set = BTreeSet::new();
    let bytes = css.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j].is_ascii_hexdigit() {
                j += 1;
            }
            let len = j - start;
            if len == 3 || len == 4 || len == 6 || len == 8 {
                set.insert(format!("#{}", css[start..j].to_ascii_lowercase()));
            }
            i = j.max(i + 1);
        } else if css[i..].starts_with("rgb(")
            || css[i..].starts_with("rgba(")
            || css[i..].starts_with("hsl(")
            || css[i..].starts_with("hsla(")
        {
            if let Some(end) = css[i..].find(')') {
                set.insert(clean(&css[i..i + end + 1]).replace(' ', ""));
                i += end + 1;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    set.into_iter().take(40).collect()
}

/// Pull `font-family` values out of CSS text.
pub fn extract_fonts(css: &str) -> Vec<String> {
    let mut set = BTreeSet::new();
    let lower = css.to_ascii_lowercase();
    let mut pos = 0;
    while let Some(idx) = lower[pos..].find("font-family") {
        let start = pos + idx + "font-family".len();
        let rest = &css[start..];
        let rest = rest.trim_start_matches([' ', ':']);
        let end = rest.find([';', '}', '\n']).unwrap_or(rest.len());
        for fam in rest[..end].split(',') {
            let f = fam.trim().trim_matches(['"', '\'']).trim();
            if !f.is_empty() && !f.starts_with("var(") {
                set.insert(f.to_string());
            }
        }
        pos = start + end;
    }
    set.into_iter().take(20).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<!doctype html><html lang="en"><head><title>Hello Site</title>
    <meta name="description" content="A test page"><link rel="stylesheet" href="/a.css">
    <style>body{color:#112233;font-family:"Inter",sans-serif} .x{background:rgb(1, 2, 3)}</style></head>
    <body><header><nav><a href="/about">About</a><a href="https://other.org">Out</a></nav></header>
    <main><h1>Welcome</h1><p>Some visible text here.</p><h2>Second</h2>
    <form action="/go" method="post"><input name="email" type="email" required placeholder="you@x"><textarea name="msg"></textarea><button type="submit">Send</button></form>
    <img src="a.png"><script>var hidden = 1;</script></main><footer>Foot</footer></body></html>"#;

    #[test]
    fn clean_room_reads_visible_only() {
        let base = Some(Url::parse("https://site.test/").unwrap());
        let p = parse_page(DOC, "https://site.test/", "/", &base, Mode::CleanRoom);
        assert_eq!(p.title.as_deref(), Some("Hello Site"));
        assert_eq!(p.headings.len(), 2);
        assert_eq!(p.headings[0].text, "Welcome");
        assert_eq!(p.links.len(), 2);
        assert!(p.links[0].internal);
        assert!(!p.links[1].internal);
        assert_eq!(p.forms.len(), 1);
        assert_eq!(p.forms[0].fields.len(), 2);
        assert_eq!(p.forms[0].submit_label.as_deref(), Some("Send"));
        assert!(p.text.contains("Some visible text"));
        assert!(!p.text.contains("hidden"));
        assert_eq!(p.image_count, 1);
        assert!(p.landmarks.contains(&"main".to_string()));
        assert!(p.colors.is_empty());
        assert!(p.stylesheets.is_empty());
    }

    #[test]
    fn deep_reads_styles() {
        let base = Some(Url::parse("https://site.test/").unwrap());
        let p = parse_page(DOC, "https://site.test/", "/", &base, Mode::Deep);
        assert_eq!(p.stylesheets, vec!["/a.css".to_string()]);
        assert!(p.colors.contains(&"#112233".to_string()));
        assert!(p.colors.contains(&"rgb(1,2,3)".to_string()));
        assert!(p.fonts.contains(&"Inter".to_string()));
    }
}
