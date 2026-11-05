//! Prompt building: spec + request + previous check → one instruction.

use aysar_core::spec::Spec;
use aysar_core::Kind;

/// What the previous check found, for refinement rounds.
pub struct PreviousCheck<'a> {
    pub score: f32,
    pub mismatches: &'a [String],
}

/// Build the instruction for a coding agent.
pub fn build_prompt(
    spec: &Spec,
    round: usize,
    previous: Option<PreviousCheck<'_>>,
    existing_files: &[String],
) -> String {
    let mut p = String::new();

    p.push_str(
        "You are the builder in Aysar, an open-source engine that studies software and recreates it from evidence.\n",
    );
    p.push_str("Work ONLY inside the current folder. Create a complete, runnable project here.\n\n");

    match spec.kind {
        Kind::Website => {
            p.push_str("## Goal\nBuild a website that matches the structure, navigation, forms and page purposes in the spec below. ");
        }
        Kind::App => {
            p.push_str(
                "## Goal\nBuild an application that matches the screens, flows and behaviours in the spec below. ",
            );
        }
        Kind::Game => {
            p.push_str("## Goal\nBuild a game that matches the gameplay rules, controls and feel in the spec below. ");
        }
        Kind::Unknown => {
            p.push_str("## Goal\nBuild a project that fulfils the request and the spec below. ");
        }
    }
    p.push_str("Write original content and assets: never copy text, logos, icons or photos from the original.\n\n");

    if let Some(stack) = &spec.stack {
        p.push_str(&format!("## Stack\nUse: **{stack}**.\n\n"));
    } else if spec.kind == Kind::Website {
        p.push_str("## Stack\nNo stack was requested. Use plain HTML, CSS and a little JavaScript, with one `index.html` at the root and one HTML file per page using the same paths as the spec (e.g. `/about` → `about/index.html` or `about.html`). No build step.\n\n");
    }

    p.push_str("## Deliverables\n");
    p.push_str("- A `README.md` that says how to run it.\n");
    if spec.kind == Kind::Website {
        p.push_str("- One HTML file per page in the spec, with the same `<title>`, headings and navigation structure. Exact files to create:\n");
        for page in &spec.pages {
            p.push_str(&format!("  - page `{}` → `{}`\n", page.path, file_for_page(&page.path)));
        }
        p.push_str("  Link between pages with these same paths (`/tools/install`, not `tools/install.html`); the checker serves the folder like a web server. The content of each page must be full, not a skeleton: every heading in the spec with a real paragraph or list under it, the word count in the same range as the original, and an image or illustration (an inline SVG you draw is fine) wherever the original has one.\n");
        p.push_str("- Semantic landmarks (`header`, `nav`, `main`, `footer`) where the spec lists them.\n");
        p.push_str("- Forms with the same fields, types and required flags; client-side validation and a visible success/error state.\n");
        p.push_str("- Responsive at 360px, 768px and 1280px widths. The result is rendered at those widths and compared with the original: match the measured colours, fonts, type scale, spacing and the height and order of header, navigation, main, forms and footer given under each page's **Layout (measured)**.\n");
    }
    if matches!(spec.kind, Kind::App | Kind::Game | Kind::Unknown) {
        p.push_str("- An `aysar-build.json` at the root: `{\"run\": \"<command that starts it>\", \"selftest\": \"<command that proves it works>\", \"notes\": \"…\"}`. The self-test must run without a display or user input, exit 0 on success, and print one line per check in the form `PASS <what>` or `FAIL <what>`.\n");
    }
    match spec.kind {
        Kind::Game => {
            p.push_str("- Gameplay first: controls, rules, scoring, win/lose conditions and restart, each covered by a `PASS`/`FAIL` line in the self-test (simulate inputs; do not require a window).\n");
            p.push_str("- A readable `GAMEPLAY.md` listing the rules you implemented and how they map to the spec.\n");
        }
        Kind::App => {
            p.push_str("- Every screen and flow in the spec, with state kept across navigation; the self-test exercises each flow headlessly.\n");
            p.push_str("- Input validation and error states for every form.\n");
        }
        _ => {}
    }
    p.push_str("- Do not ask questions; make sensible choices and note them in `DECISIONS.md`.\n\n");

    if round > 1 {
        p.push_str(&format!("## Round {round}: refine, do not start over\n"));
        if !existing_files.is_empty() {
            p.push_str("The project already exists. Files present:\n");
            for f in existing_files.iter().take(80) {
                p.push_str(&format!("- {f}\n"));
            }
            if existing_files.len() > 80 {
                p.push_str(&format!("- … and {} more\n", existing_files.len() - 80));
            }
        }
        if let Some(prev) = previous {
            p.push_str(&format!("\nThe last check scored **{:.0}/100**. Fix these mismatches:\n", prev.score));
            for m in prev.mismatches.iter().take(40) {
                p.push_str(&format!("- {m}\n"));
            }
        }
        p.push('\n');
    }

    p.push_str("---\n\n");
    p.push_str(&spec.to_markdown());
    p
}

/// The file a page path should live in: `/` → `index.html`, `/learn` → `learn.html`,
/// `/tools/install` → `tools/install.html`. The checker's static server resolves both
/// `x.html` and `x/index.html`, so either works; this is the simpler one to ask for.
pub fn file_for_page(path: &str) -> String {
    let p = path.trim().trim_matches('/');
    if p.is_empty() {
        "index.html".into()
    } else if p.ends_with(".html") || p.ends_with(".htm") {
        p.to_string()
    } else {
        format!("{p}.html")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aysar_core::spec::{DesignSystem, Rules};

    #[test]
    fn pages_map_to_files() {
        assert_eq!(file_for_page("/"), "index.html");
        assert_eq!(file_for_page("/learn"), "learn.html");
        assert_eq!(file_for_page("/tools/install/"), "tools/install.html");
        assert_eq!(file_for_page("/about.html"), "about.html");
    }

    #[test]
    fn prompt_lists_the_exact_files() {
        let mut spec = Spec {
            spec_version: 1,
            name: "t".into(),
            kind: Kind::Website,
            request: "Recreate".into(),
            source: "x".into(),
            mode: "clean-room".into(),
            generated_at: String::new(),
            stack: None,
            pages: vec![],
            design: DesignSystem::default(),
            behaviors: vec![],
            rules: Rules::default(),
            program: None,
            limitations: vec![],
        };
        for path in ["/", "/tools/install", "/learn"] {
            spec.pages.push(aysar_core::spec::PageSpec { path: path.into(), ..Default::default() });
        }
        let p = build_prompt(&spec, 1, None, &[]);
        assert!(p.contains("page `/tools/install` → `tools/install.html`"), "{p}");
        assert!(p.contains("page `/` → `index.html`"));
    }

    #[test]
    fn prompt_has_sections() {
        let spec = Spec {
            spec_version: 1,
            name: "t".into(),
            kind: Kind::Website,
            request: "Recreate".into(),
            source: "x".into(),
            mode: "clean-room".into(),
            generated_at: String::new(),
            stack: None,
            pages: vec![],
            design: DesignSystem::default(),
            behaviors: vec![],
            rules: Rules::default(),
            program: None,
            limitations: vec![],
        };
        let p = build_prompt(
            &spec,
            2,
            Some(PreviousCheck { score: 40.0, mismatches: &["title differs".into()] }),
            &["index.html".into()],
        );
        assert!(p.contains("Round 2"));
        let mut game = spec.clone();
        game.kind = Kind::Game;
        let g = build_prompt(&game, 1, None, &[]);
        assert!(g.contains("aysar-build.json"));
        assert!(g.contains("GAMEPLAY.md"));
        assert!(p.contains("title differs"));
        assert!(p.contains("# Spec: t"));
    }
}
