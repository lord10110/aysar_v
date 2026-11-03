//! A small MCP server over stdio. No framework: newline-delimited JSON-RPC
//! 2.0, the `initialize`, `tools/list` and `tools/call` methods, and
//! `notifications/initialized`. Enough for Claude Code, Codex, Cursor and
//! any other MCP client to call Aysar.
//!
//! Tools:
//! - `aysar_study`: observe a URL or local path, return a summary + capture.
//! - `aysar_identify`: say what a local file is (magic bytes + extension).
//! - `aysar_rebuild`: run the whole pipeline for a target and request.
//! - `aysar_check`: check an existing project again.
//! - `aysar_doctor`: report installed agents and engines.

use aysar_capture::CaptureOptions;
use aysar_core::project::Project;
use aysar_core::{Kind, Mode, Target};
use aysar_orchestrator::{Quiet, RebuildOptions};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::Path;

pub const PROTOCOL_VERSION: &str = "2024-11-05";

pub fn tool_list() -> Value {
    json!([
        {
            "name": "aysar_study",
            "description": "Observe a website (URL) or local HTML file/folder. Clean room by default: only what a visitor sees. Returns a summary and the capture.",
            "inputSchema": {"type":"object","properties":{
                "target":{"type":"string","description":"URL or local path"},
                "deep":{"type":"boolean","default":false},
                "max_pages":{"type":"integer","default":5},
                "render":{"type":"boolean","default":true,"description":"Render in headless Chromium for screenshots, measured styles and layout"}
            },"required":["target"]}
        },
        {
            "name": "aysar_identify",
            "description": "Say what a local file is, from its magic bytes and extension. Does not read its contents further.",
            "inputSchema": {"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}
        },
        {
            "name": "aysar_rebuild",
            "description": "Study a target, write a spec, have a coding agent build a new project, check it against the original and refine. Returns the project folder and the last check.",
            "inputSchema": {"type":"object","properties":{
                "target":{"type":"string","description":"URL, local path, or '-' for none"},
                "request":{"type":"string"},
                "deep":{"type":"boolean","default":false},
                "agent":{"type":"string","description":"claude-code | codex | dry-run | command template"},
                "rounds":{"type":"integer","default":2},
                "projects_dir":{"type":"string"}
            },"required":["target","request"]}
        },
        {
            "name": "aysar_check",
            "description": "Re-check an existing Aysar project folder against its captured original.",
            "inputSchema": {"type":"object","properties":{"project":{"type":"string"}},"required":["project"]}
        },
        {
            "name": "aysar_doctor",
            "description": "Report which coding agents and analysis engines are installed.",
            "inputSchema": {"type":"object","properties":{}}
        }
    ])
}

fn text_result(text: String, structured: Option<Value>) -> Value {
    let mut r = json!({"content":[{"type":"text","text":text}]});
    if let Some(s) = structured {
        r["structuredContent"] = s;
    }
    r
}

/// Handle one `tools/call`.
pub fn call_tool(name: &str, args: &Value) -> anyhow::Result<Value> {
    let s = |k: &str| args.get(k).and_then(|v| v.as_str()).map(|v| v.to_string());
    let b = |k: &str| args.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let n = |k: &str, d: u64| args.get(k).and_then(|v| v.as_u64()).unwrap_or(d) as usize;

    match name {
        "aysar_study" => {
            let target = Target::parse(&s("target").ok_or_else(|| anyhow::anyhow!("target is required"))?);
            let mode = if b("deep") { Mode::Deep } else { Mode::CleanRoom };
            let cap = aysar_capture::capture(
                &target,
                &CaptureOptions { mode, max_pages: n("max_pages", 5), ..Default::default() },
            )?;
            Ok(text_result(aysar_capture::summarise(&cap), Some(serde_json::to_value(&cap)?)))
        }
        "aysar_identify" => {
            let p = s("path").ok_or_else(|| anyhow::anyhow!("path is required"))?;
            let what = aysar_analysis::identify(Path::new(&p))?;
            Ok(text_result(format!("{p}: {what}"), Some(json!({"path":p,"kind":what}))))
        }
        "aysar_rebuild" => {
            let target = Target::parse(&s("target").ok_or_else(|| anyhow::anyhow!("target is required"))?);
            let request = s("request").ok_or_else(|| anyhow::anyhow!("request is required"))?;
            let mode = if b("deep") { Mode::Deep } else { Mode::CleanRoom };
            let dir = s("projects_dir")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(aysar_orchestrator::default_projects_dir);
            let kind = if matches!(target, Target::None) { Some(guess_kind(&request)) } else { None };
            let mut p = aysar_orchestrator::new_project(&dir, target, &request, mode, kind)?;
            let opts = RebuildOptions { mode, agent: s("agent"), rounds: n("rounds", 2), ..Default::default() };
            let report = aysar_orchestrator::rebuild(&mut p, &opts, &Quiet)?;
            let text = match &report {
                Some(r) => format!("Project: {}\nScore: {:.0}/100\n{}", p.root.display(), r.score, r.to_markdown()),
                None => format!(
                    "Project: {}\nDry run: prompt written to app/BUILD-PROMPT.md (no agent installed).",
                    p.root.display()
                ),
            };
            Ok(text_result(text, Some(json!({"project": p.root.display().to_string(), "report": report}))))
        }
        "aysar_check" => {
            let root = s("project").ok_or_else(|| anyhow::anyhow!("project is required"))?;
            let mut p = Project::open(&root)?;
            let r = aysar_orchestrator::check(&mut p, &Quiet)?;
            Ok(text_result(r.to_markdown(), Some(serde_json::to_value(&r)?)))
        }
        "aysar_doctor" => {
            let agents = aysar_build::status();
            let engine = aysar_analysis::detect();
            let mut text = String::from("Agents:\n");
            for (name, ok) in &agents {
                text.push_str(&format!("  {} {}\n", if *ok { "✓" } else { "✗" }, name));
            }
            text.push_str(&format!("Engine: {} {}\n", engine.name(), engine.version().unwrap_or_default()));
            let chrome = aysar_render::find_chrome();
            text.push_str(&format!(
                "Renderer: {}\n",
                chrome.as_ref().map(|c| c.display().to_string()).unwrap_or_else(|| "none".into())
            ));
            Ok(text_result(text, Some(json!({"agents": agents, "engine": engine.name(), "chrome": chrome}))))
        }
        other => anyhow::bail!("unknown tool {other}"),
    }
}

fn guess_kind(request: &str) -> Kind {
    let r = request.to_ascii_lowercase();
    if r.contains("game") || r.contains("لعبة") {
        Kind::Game
    } else if r.contains("app") || r.contains("تطبيق") {
        Kind::App
    } else if r.contains("site") || r.contains("web") || r.contains("page") || r.contains("موقع") {
        Kind::Website
    } else {
        Kind::Unknown
    }
}

/// Handle one JSON-RPC request. Returns `None` for notifications.
pub fn handle(req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "aysar", "version": env!("CARGO_PKG_VERSION")}
        })),
        "notifications/initialized" | "notifications/cancelled" => return None,
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_list()})),
        "tools/call" => {
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match call_tool(name, &args) {
                Ok(v) => Ok(v),
                Err(e) => Ok(json!({"content":[{"type":"text","text":format!("error: {e}")}],"isError":true})),
            }
        }
        _ => Err((-32601, format!("method not found: {method}"))),
    };

    let id = id?;
    Some(match result {
        Ok(r) => json!({"jsonrpc":"2.0","id":id,"result":r}),
        Err((code, msg)) => json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":msg}}),
    })
}

/// Serve on stdin/stdout until EOF.
pub fn serve_stdio() -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let err =
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":format!("parse error: {e}")}});
                writeln!(stdout, "{err}")?;
                stdout.flush()?;
                continue;
            }
        };
        if let Some(resp) = handle(&req) {
            writeln!(stdout, "{resp}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_and_list() {
        let r = handle(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})).unwrap();
        assert_eq!(r["result"]["serverInfo"]["name"], "aysar");
        let r = handle(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
        assert_eq!(r["result"]["tools"].as_array().unwrap().len(), 5);
        assert!(handle(&json!({"jsonrpc":"2.0","method":"notifications/initialized"})).is_none());
    }

    #[test]
    fn doctor_call() {
        let r = handle(
            &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"aysar_doctor","arguments":{}}}),
        )
        .unwrap();
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("Agents"));
    }
}
