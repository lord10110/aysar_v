//! Engine-driven study of a program (Deep mode only).
//!
//! Runs a few engine tools, routed by what the file is, and distils them into
//! a [`ProgramStudy`]: what it is, what it depends on, how big it is, which
//! strings look like text a person would see, which names reveal features.
//! No code or data is copied; the spec gets names, counts and short samples.
//!
//! Tool names follow craft-anything's `docs/tools.md`; each call is also a
//! line in `tools_run`, so the evidence says exactly what was consulted.

use crate::{Engine, ToolOutput};
use aysar_core::capture::ProgramStudy;
use aysar_core::Mode;
use serde_json::Value;

/// Study `path` with `engine`. Never called in Clean room by the orchestrator;
/// refuses anyway if it is.
pub fn study_program(engine: &dyn Engine, path: &str, mode: Mode) -> anyhow::Result<ProgramStudy> {
    if !mode.is_deep() {
        anyhow::bail!("Clean room: a program's insides are not read. Use --deep to study it with the analysis engine.");
    }
    let mut st = ProgramStudy {
        engine: format!("{} {}", engine.name(), engine.version().unwrap_or_default()).trim().to_string(),
        ..Default::default()
    };
    let p = path.to_string();

    // 1. What is this?
    let triage = run(engine, &mut st, "core_target_triage", std::slice::from_ref(&p), mode);
    let (format, kind) = match &triage {
        Some(t) => {
            let r = &t.json["result"];
            st.format = r["format"].as_str().map(|s| s.to_string());
            st.platform = r["operating_system"].as_str().map(|s| s.to_string());
            st.arch = r["architecture"].as_str().map(|s| s.to_string());
            st.what = r["plain_english"]
                .as_str()
                .map(|s| first_sentences(s, 2))
                .or_else(|| r["format_name"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown".into());
            for l in r["libraries"].as_array().into_iter().flatten() {
                if let Some(s) = l.as_str() {
                    st.dependencies.push(s.to_string());
                }
            }
            (st.format.clone().unwrap_or_default(), r["kind"].as_str().unwrap_or("").to_string())
        }
        None => (String::new(), String::new()),
    };
    if let Ok(meta) = std::fs::metadata(path) {
        st.size_bytes = Some(meta.len());
    }

    // 2. Route by format.
    match format.as_str() {
        "apk" | "android" => {
            if let Some(o) = run(engine, &mut st, "android_package_show", std::slice::from_ref(&p), mode) {
                let r = &o.json["result"];
                st.what = r["plain_english"].as_str().map(|s| first_sentences(s, 2)).unwrap_or(st.what.clone());
                collect_strs(&r["permissions"], &mut st.feature_names, 30);
            }
            if let Some(o) = run(engine, &mut st, "android_components_list", std::slice::from_ref(&p), mode) {
                collect_names(
                    &o.json["result"],
                    &["activities", "services", "receivers", "components"],
                    &mut st.components,
                    60,
                );
            }
            if let Some(o) = run(engine, &mut st, "android_resources_read", std::slice::from_ref(&p), mode) {
                collect_names(
                    &o.json["result"],
                    &["resources", "strings", "layouts", "drawables"],
                    &mut st.resources,
                    80,
                );
                collect_ui_strings_from_values(&o.json["result"], &mut st.ui_strings, 120);
            }
        }
        "jar" | "java" => {
            if let Some(o) = run(engine, &mut st, "java_archive_show", std::slice::from_ref(&p), mode) {
                let r = &o.json["result"];
                st.what = r["plain_english"].as_str().map(|s| first_sentences(s, 2)).unwrap_or(st.what.clone());
                collect_names(r, &["classes", "packages", "entry_points"], &mut st.components, 60);
            }
        }
        "dotnet" | "pe" if kind.contains("dotnet") || st.what.to_ascii_lowercase().contains(".net") => {
            if let Some(o) = run(engine, &mut st, "managed_assembly_show", std::slice::from_ref(&p), mode) {
                let r = &o.json["result"];
                collect_names(r, &["references", "dependencies"], &mut st.dependencies, 40);
            }
            if let Some(o) = run(engine, &mut st, "managed_members_list", std::slice::from_ref(&p), mode) {
                collect_names(&o.json["result"], &["types", "members"], &mut st.feature_names, 80);
            }
            native_strings(engine, &mut st, &p, mode);
        }
        "macho" => {
            if let Some(o) = run(engine, &mut st, "apple_objc_classes_list", std::slice::from_ref(&p), mode) {
                collect_names(&o.json["result"], &["classes"], &mut st.feature_names, 80);
            }
            native_overview_and_strings(engine, &mut st, &p, mode);
        }
        "pe" => {
            if let Some(o) = run(engine, &mut st, "native_pe_resources_list", std::slice::from_ref(&p), mode) {
                collect_names(&o.json["result"], &["resources", "entries"], &mut st.resources, 60);
            }
            native_overview_and_strings(engine, &mut st, &p, mode);
        }
        _ => {
            native_overview_and_strings(engine, &mut st, &p, mode);
        }
    }

    if st.what.is_empty() {
        st.what = "unknown".into();
    }
    if st.tools_run.is_empty() {
        st.limitations.push("The analysis engine ran no tools; is craft-anything-cli installed and on PATH?".into());
    }
    Ok(st)
}

fn native_overview_and_strings(engine: &dyn Engine, st: &mut ProgramStudy, p: &str, mode: Mode) {
    if let Some(o) = run(engine, st, "core_overview_show", &[p.to_string()], mode) {
        let r = &o.json["result"];
        st.function_count = r["counts"]["functions"].as_u64().map(|n| n as usize);
        st.string_count = r["counts"]["strings"].as_u64().map(|n| n as usize);
        for l in o.json["limitations"].as_array().into_iter().flatten() {
            if let Some(m) = l["message"].as_str() {
                st.limitations.push(m.to_string());
            }
        }
    }
    if let Some(o) = run(engine, st, "native_functions_list", &[p.to_string(), "--limit".into(), "2000".into()], mode) {
        for f in o.json["result"]["functions"].as_array().into_iter().flatten() {
            let name = f["name"].as_str().unwrap_or("");
            let generated = f["name_source"].as_str() == Some("generated") || name.starts_with("sub_");
            if generated || name.is_empty() || name == "entry" {
                continue;
            }
            // Imports (`foo@plt`, `__imp_foo`) say what the program *uses*; real
            // function names say what it *does*.
            if let Some(imp) = name.strip_suffix("@plt").or_else(|| name.strip_prefix("__imp_")) {
                let imp = imp.trim_start_matches('_');
                if !imp.is_empty() && !st.imports.contains(&imp.to_string()) && st.imports.len() < 60 {
                    st.imports.push(imp.to_string());
                }
            } else if !name.starts_with("__") && st.feature_names.len() < 80 {
                st.feature_names.push(name.to_string());
            }
        }
    }
    native_strings(engine, st, p, mode);
}

fn native_strings(engine: &dyn Engine, st: &mut ProgramStudy, p: &str, mode: Mode) {
    // Page through up to 5000 strings; keep the ones that read like UI text.
    let mut offset = 0usize;
    let page = 1000usize;
    while offset < 5000 {
        let args = vec![p.to_string(), "--limit".into(), page.to_string(), "--offset".into(), offset.to_string()];
        let Some(o) = run(engine, st, "native_strings_browse", &args, mode) else { break };
        let items = o.json["result"]["strings"].as_array().cloned().unwrap_or_default();
        if st.string_count.is_none() {
            st.string_count = o.json["result"]["total"].as_u64().map(|n| n as usize);
        }
        for it in &items {
            if let Some(raw) = it["text"].as_str() {
                let t = raw.split_whitespace().collect::<Vec<_>>().join(" ");
                if looks_like_ui_text(&t) && !st.ui_strings.contains(&t) {
                    st.ui_strings.push(t);
                    if st.ui_strings.len() >= 150 {
                        return;
                    }
                }
            }
        }
        if items.len() < page {
            break;
        }
        offset += page;
    }
}

/// Run one tool; record it; swallow failures into `limitations`.
fn run(engine: &dyn Engine, st: &mut ProgramStudy, tool: &str, args: &[String], mode: Mode) -> Option<ToolOutput> {
    let cli_name = tool.replace('_', "-");
    let mut full = args.to_vec();
    full.push("--json".into());
    match engine.run(&cli_name, &full, mode) {
        Ok(o) if o.ok && !o.json.is_null() => {
            st.tools_run.push(tool.to_string());
            Some(o)
        }
        Ok(o) => {
            let why = o.stderr.lines().next().unwrap_or("no JSON result").to_string();
            st.limitations.push(format!("{tool}: {why}"));
            None
        }
        Err(e) => {
            st.limitations.push(format!("{tool}: {e}"));
            None
        }
    }
}

/// Heuristic: text a person would read in the UI, not an identifier or a path.
pub fn looks_like_ui_text(s: &str) -> bool {
    let t = s.trim();
    let n = t.chars().count();
    if !(4..=120).contains(&n) {
        return false;
    }
    let words: Vec<&str> = t.split_whitespace().collect();
    if words.len() < 2 || words.len() > 20 {
        return false;
    }
    let alpha = t.chars().filter(|c| c.is_alphabetic()).count();
    if (alpha as f32) / (n as f32) < 0.6 {
        return false;
    }
    // Identifiers, paths, format strings, Rust/C++ internals.
    if t.contains("::")
        || t.contains('/')
        || t.contains('\\')
        || t.contains('%')
        || t.contains('{')
        || t.contains('<')
        || t.contains('_')
    {
        return false;
    }
    let first = t.chars().next().unwrap_or(' ');
    if !first.is_uppercase() && !first.is_alphabetic() {
        return false;
    }
    // Lower-case-only runs of camelCase identifiers are not sentences.
    let has_lower_word = words.iter().any(|w| w.chars().all(|c| c.is_lowercase()));
    has_lower_word || t.ends_with('.') || t.ends_with('!') || t.ends_with('?')
}

fn first_sentences(s: &str, n: usize) -> String {
    let mut out = String::new();
    for (i, part) in s.split_inclusive(['.', '!', '?']).enumerate() {
        if i >= n {
            break;
        }
        out.push_str(part);
    }
    out.trim().to_string()
}

fn collect_strs(v: &Value, out: &mut Vec<String>, max: usize) {
    for x in v.as_array().into_iter().flatten() {
        if out.len() >= max {
            break;
        }
        if let Some(s) = x.as_str() {
            out.push(s.to_string());
        } else if let Some(s) = x["name"].as_str() {
            out.push(s.to_string());
        }
    }
}

fn collect_names(r: &Value, keys: &[&str], out: &mut Vec<String>, max: usize) {
    for k in keys {
        collect_strs(&r[*k], out, max);
    }
}

fn collect_ui_strings_from_values(r: &Value, out: &mut Vec<String>, max: usize) {
    fn walk(v: &Value, out: &mut Vec<String>, max: usize) {
        if out.len() >= max {
            return;
        }
        match v {
            Value::String(s) => {
                if looks_like_ui_text(s) && !out.contains(s) {
                    out.push(s.clone());
                }
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out, max)),
            Value::Object(o) => o.values().for_each(|x| walk(x, out, max)),
            _ => {}
        }
    }
    walk(r, out, max);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_text_heuristic() {
        assert!(looks_like_ui_text("Welcome back to your dashboard"));
        assert!(looks_like_ui_text("Could not save the file."));
        assert!(!looks_like_ui_text("BoundedBacktrackerEngine"));
        assert!(!looks_like_ui_text("core::fmt::Display"));
        assert!(!looks_like_ui_text("/lib64/ld-linux-x86-64.so.2"));
        assert!(!looks_like_ui_text("%s: %d"));
        assert!(!looks_like_ui_text("ok"));
    }

    #[test]
    fn clean_room_refuses() {
        let err = study_program(&crate::NullEngine, "/bin/ls", Mode::CleanRoom).unwrap_err();
        assert!(err.to_string().contains("Clean room"));
    }

    #[test]
    fn null_engine_reports_limitation() {
        let st = study_program(&crate::NullEngine, "/bin/ls", Mode::Deep).unwrap();
        assert!(st.tools_run.is_empty());
        assert!(!st.limitations.is_empty());
    }

    /// Runs only when craft-anything-cli is installed.
    #[test]
    fn live_study_if_engine() {
        let ca = crate::CraftAnything::default();
        if !ca.available() {
            eprintln!("no craft-anything-cli; skipping");
            return;
        }
        let me = std::env::current_exe().unwrap();
        let st = study_program(&ca, me.to_str().unwrap(), Mode::Deep).unwrap();
        assert!(st.tools_run.contains(&"core_target_triage".to_string()), "{:?}", st.tools_run);
        assert_eq!(st.format.as_deref(), Some("elf"));
        assert!(st.function_count.unwrap_or(0) > 10);
        assert!(!st.what.is_empty());
    }
}
