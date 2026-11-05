//! The built-in builder: no coding-agent program needed.
//!
//! Talks to any OpenAI-compatible chat endpoint — a local model through
//! Ollama, LM Studio or llama.cpp, or a hosted one (OpenAI, OpenRouter,
//! Groq, …) — asks for the whole project as a set of files, and writes them
//! into `app/`. Refinement rounds send the current files back with the
//! check's mismatches.
//!
//! The file protocol is plain text, so small local models can follow it:
//!
//! ```text
//! <<<FILE index.html>>>
//! …whole file…
//! <<<END>>>
//! <<<DELETE old.css>>>
//! ```

use crate::{list_files, Agent, BuildOutput};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

/// Settings, from `AYSAR_LLM_URL` / `AYSAR_LLM_MODEL` / `AYSAR_LLM_KEY`
/// (the same ones `aysar enrich` uses) with Ollama defaults.
#[derive(Debug, Clone)]
pub struct ApiAgent {
    pub url: String,
    pub model: String,
    pub key: Option<String>,
    /// Give up when the model sends nothing for this long (`AYSAR_LLM_IDLE_TIMEOUT`, seconds).
    /// The answer streams in, so a slow model is never cut off while it is still typing.
    pub timeout: Duration,
    /// Hard ceiling for one answer (`AYSAR_LLM_TIMEOUT`, seconds).
    pub max_total: Duration,
    /// Largest file set we will send back for refinement, in bytes.
    pub max_context_bytes: usize,
}

fn secs_env(name: &str, default: u64) -> Duration {
    Duration::from_secs(std::env::var(name).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default))
}

impl Default for ApiAgent {
    fn default() -> Self {
        ApiAgent {
            url: std::env::var("AYSAR_LLM_URL").unwrap_or_else(|_| "http://localhost:11434/v1".into()),
            model: std::env::var("AYSAR_LLM_MODEL").unwrap_or_else(|_| "qwen2.5-coder:7b".into()),
            key: std::env::var("AYSAR_LLM_KEY").ok().filter(|k| !k.is_empty()),
            timeout: secs_env("AYSAR_LLM_IDLE_TIMEOUT", 300),
            max_total: secs_env("AYSAR_LLM_TIMEOUT", 3 * 3600),
            max_context_bytes: 120_000,
        }
    }
}

const SYSTEM: &str = "You are the builder inside Aysar. You produce a complete, runnable project as plain files.\n\
OUTPUT FORMAT, and nothing else before or after it:\n\
<<<FILE relative/path.ext>>>\n(the whole file)\n<<<END>>>\n\
Repeat one block per file. To remove a file: <<<DELETE relative/path.ext>>>\n\
Rules: every file is complete (never \"…\" or \"rest unchanged\"); paths are relative, forward slashes, no leading slash; \
no explanations, no markdown fences around the blocks; include README.md and DECISIONS.md; \
never copy text, logos or images from the original — write original content that serves the same purpose.";

const REMINDER: &str = "\n\n---\nOUTPUT FORMAT REMINDER: reply with the files only, each as\n<<<FILE relative/path.ext>>>\n(whole file)\n<<<END>>>\nNo prose, no ``` fences, no file left out. Start with <<<FILE index.html>>> for a website.";

const CORRECTION: &str = "Your reply contained no <<<FILE path>>> … <<<END>>> blocks, so no files could be written. \
Send the complete project again, every file in full, using exactly that format and nothing else.";

impl ApiAgent {
    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.url.trim_end_matches('/'))
    }

    fn chat(&self, messages: Vec<Value>) -> anyhow::Result<String> {
        let body = json!({ "model": self.model, "temperature": 0.2, "messages": messages, "stream": true });
        let agent =
            ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout_read(self.timeout).build();
        let mut req = agent.post(&self.endpoint()).set("Content-Type", "application/json");
        if let Some(k) = &self.key {
            req = req.set("Authorization", &format!("Bearer {k}"));
        }
        let resp = match req.send_json(body) {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let text = r.into_string().unwrap_or_default();
                anyhow::bail!("{} answered HTTP {code}: {}", self.endpoint(), crate::tail_of(&text, 400));
            }
            Err(e) => anyhow::bail!("could not reach {} ({e}); is the model server running?", self.endpoint()),
        };
        let streamed = resp.header("Content-Type").map(|c| c.contains("text/event-stream")).unwrap_or(false);
        let start = Instant::now();
        let reader = std::io::BufReader::new(resp.into_reader());
        let mut text = String::new();
        let mut raw = String::new();
        let mut last_note = Instant::now();
        use std::io::BufRead;
        for line in reader.lines() {
            if last_note.elapsed() > Duration::from_secs(60) {
                last_note = Instant::now();
                eprintln!(
                    "    {} is still writing: {} characters after {} s",
                    self.model,
                    text.len(),
                    start.elapsed().as_secs()
                );
            }
            let line = match line {
                Ok(l) => l,
                Err(e) => anyhow::bail!(
                    "the model stopped answering ({e}); it sent nothing for {} s. A smaller site (--max-pages), a smaller model, or AYSAR_LLM_IDLE_TIMEOUT can help{}",
                    self.timeout.as_secs(),
                    if text.is_empty() { String::new() } else { format!("; {} characters had arrived", text.len()) }
                ),
            };
            if start.elapsed() > self.max_total {
                anyhow::bail!(
                    "the model has been answering for {} s (AYSAR_LLM_TIMEOUT); giving up",
                    start.elapsed().as_secs()
                );
            }
            if !streamed {
                raw.push_str(&line);
                continue;
            }
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data == "[DONE]" {
                break;
            }
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                if let Some(piece) = v["choices"][0]["delta"]["content"].as_str() {
                    text.push_str(piece);
                } else if let Some(whole) = v["choices"][0]["message"]["content"].as_str() {
                    text.push_str(whole);
                }
                if let Some(err) = v["error"]["message"].as_str() {
                    anyhow::bail!("{} reported: {err}", self.endpoint());
                }
            }
        }
        if !streamed {
            // A server that ignored `stream: true` and sent one JSON object.
            let v: Value = serde_json::from_str(&raw)?;
            return v["choices"][0]["message"]["content"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("no message content in the model's reply"));
        }
        if text.is_empty() {
            anyhow::bail!("the model's reply was empty");
        }
        Ok(text)
    }

    /// Current files of the project, for refinement rounds, within the budget.
    fn snapshot(&self, workdir: &Path) -> String {
        let mut out = String::new();
        let mut used = 0usize;
        for rel in list_files(workdir) {
            if rel == "BUILD-PROMPT.md" || rel.starts_with(".aysar") {
                continue;
            }
            let Ok(text) = fs::read_to_string(workdir.join(&rel)) else { continue };
            if used + text.len() > self.max_context_bytes {
                out.push_str(&format!("<<<FILE {rel}>>>\n(omitted: too large to resend)\n<<<END>>>\n"));
                continue;
            }
            used += text.len();
            out.push_str(&format!("<<<FILE {rel}>>>\n{text}\n<<<END>>>\n"));
        }
        out
    }
}

impl Agent for ApiAgent {
    fn name(&self) -> &str {
        "api"
    }

    /// Reachable if the server lists models (or at least answers).
    fn available(&self) -> bool {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(3)).build();
        let url = format!("{}/models", self.url.trim_end_matches('/'));
        let mut req = agent.get(&url);
        if let Some(k) = &self.key {
            req = req.set("Authorization", &format!("Bearer {k}"));
        }
        matches!(req.call(), Ok(_) | Err(ureq::Error::Status(_, _)))
    }

    fn run(&self, workdir: &Path, prompt: &str) -> anyhow::Result<BuildOutput> {
        fs::create_dir_all(workdir)?;
        let start = Instant::now();
        let existing = self.snapshot(workdir);
        let mut user = String::from(prompt);
        if !existing.is_empty() {
            user.push_str("\n\n---\nCURRENT FILES (resend any file you change, complete):\n\n");
            user.push_str(&existing);
        }
        // Small models follow the last instruction best: repeat the format at the end.
        user.push_str(REMINDER);
        let mut messages = vec![json!({"role":"system","content":SYSTEM}), json!({"role":"user","content":user})];
        let mut reply = self.chat(messages.clone())?;
        let (mut written, mut deleted, mut note) = apply_reply(workdir, &reply)?;
        if written == 0 && deleted == 0 {
            // One correction: show the model its own reply and ask again in the protocol.
            messages.push(json!({"role":"assistant","content":reply}));
            messages.push(json!({"role":"user","content":CORRECTION}));
            reply = self.chat(messages)?;
            let (w, d, n) = apply_reply(workdir, &reply)?;
            written = w;
            deleted = d;
            note = if n.is_empty() { "after one correction".into() } else { format!("after one correction; {n}") };
        }
        let ok = written > 0;
        Ok(BuildOutput {
            agent: "api".into(),
            ok,
            seconds: start.elapsed().as_secs_f64(),
            stdout_tail: format!(
                "{} via {}: wrote {written} file(s), deleted {deleted}{}",
                self.model,
                self.endpoint(),
                if note.is_empty() { String::new() } else { format!("; {note}") }
            ),
            stderr_tail: if ok { String::new() } else { crate::tail_of(&reply, 1500) },
            files: list_files(workdir),
        })
    }
}

/// Parse the file protocol and write to disk. Returns (written, deleted, note).
pub fn apply_reply(workdir: &Path, reply: &str) -> anyhow::Result<(usize, usize, String)> {
    let mut written = 0;
    let mut deleted = 0;
    let mut notes = Vec::new();
    let mut current: Option<(String, String)> = None;

    for line in reply.lines() {
        let l = line.trim_end();
        if let Some(rest) = l.trim_start().strip_prefix("<<<FILE ") {
            if let Some((path, body)) = current.take() {
                if write_file(workdir, &path, &body, &mut notes)? {
                    written += 1;
                }
            }
            let path = rest.trim_end_matches(">>>").trim().to_string();
            current = Some((path, String::new()));
        } else if l.trim() == "<<<END>>>" {
            if let Some((path, body)) = current.take() {
                if write_file(workdir, &path, &body, &mut notes)? {
                    written += 1;
                }
            }
        } else if let Some(rest) = l.trim_start().strip_prefix("<<<DELETE ") {
            let path = rest.trim_end_matches(">>>").trim();
            if safe_path(path) && fs::remove_file(workdir.join(path)).is_ok() {
                deleted += 1;
            }
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some((path, body)) = current.take() {
        // Unterminated last block: keep it, note it.
        if write_file(workdir, &path, &body, &mut notes)? {
            written += 1;
            notes.push(format!("`{path}` had no <<<END>>>; kept as sent"));
        }
    }
    if written == 0 && deleted == 0 {
        // Fallback: files given as Markdown fences (`### index.html` + ```html …```).
        let fenced = crate::markdown_files::parse(reply);
        let n = fenced.len();
        for (path, body) in fenced {
            if write_file(workdir, &path, &body, &mut notes)? {
                written += 1;
            }
        }
        if written > 0 {
            notes.push(format!(
                "the model answered in Markdown fences instead of the file protocol; {written} of {n} accepted"
            ));
        } else {
            notes.push("the model sent no <<<FILE …>>> blocks and no fenced files; its reply is in stderr".into());
        }
    }
    Ok((written, deleted, notes.join("; ")))
}

fn write_file(workdir: &Path, path: &str, body: &str, notes: &mut Vec<String>) -> anyhow::Result<bool> {
    if !safe_path(path) {
        notes.push(format!("skipped unsafe path `{path}`"));
        return Ok(false);
    }
    // Strip a stray fence the model may have wrapped the file in.
    let mut text = body.to_string();
    let trimmed = text.trim_start();
    if trimmed.starts_with("```") {
        if let Some(nl) = trimmed.find('\n') {
            text = trimmed[nl + 1..].to_string();
        }
        if let Some(end) = text.trim_end().strip_suffix("```") {
            text = end.to_string();
        }
    }
    let full = workdir.join(path);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(full, text.trim_end_matches('\n').to_string() + "\n")?;
    Ok(true)
}

/// Relative, inside the project, no traversal, no absolute or drive paths.
pub fn safe_path(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.starts_with('\\')
        && !p.contains("..")
        && !p.contains(':')
        && !p.contains('\0')
        && p.len() < 300
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_writes_files() {
        let dir = std::env::temp_dir().join(format!("aysar-api-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("old.txt"), "x").unwrap();
        let reply = "Here you go:\n<<<FILE index.html>>>\n```html\n<h1>Hi</h1>\n```\n<<<END>>>\n<<<FILE css/site.css>>>\nbody{margin:0}\n<<<END>>>\n<<<DELETE old.txt>>>\n<<<FILE ../../etc/passwd>>>\nnope\n<<<END>>>\n";
        let (w, d, note) = apply_reply(&dir, reply).unwrap();
        assert_eq!(w, 2);
        assert_eq!(d, 1);
        assert!(note.contains("unsafe"));
        assert_eq!(fs::read_to_string(dir.join("index.html")).unwrap(), "<h1>Hi</h1>\n");
        assert!(dir.join("css").join("site.css").exists());
        assert!(!dir.join("old.txt").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn accepts_markdown_fences_when_the_protocol_is_ignored() {
        let dir = std::env::temp_dir().join(format!("aysar-api-md-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let reply = "Here is the website:\n\n### index.html\n```html\n<!doctype html>\n<link rel=\"stylesheet\" href=\"styles.css\">\n<script src=\"scripts.js\"></script>\n```\n\n### styles.css\n```css\nbody{margin:0}\n```\n\n```javascript\nconsole.log('hi')\n```\n";
        let (w, d, note) = apply_reply(&dir, reply).unwrap();
        assert_eq!((w, d), (3, 0));
        assert!(note.contains("Markdown fences"), "{note}");
        assert!(dir.join("index.html").exists());
        assert!(dir.join("styles.css").exists());
        assert!(dir.join("scripts.js").exists(), "nameless js fence takes the name the HTML links");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn retries_once_when_the_first_reply_has_no_files() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let handle = std::thread::spawn(move || {
            let mut n = 0;
            for _ in 0..2 {
                let mut req = server.recv().unwrap();
                assert_eq!(req.url(), "/v1/chat/completions");
                let mut body = String::new();
                req.as_reader().read_to_string(&mut body).unwrap();
                n += 1;
                let content = if n == 1 {
                    assert!(!body.contains("contained no <<<FILE"));
                    "Sorry, I can only describe the site."
                } else {
                    assert!(body.contains("contained no <<<FILE"), "second call carries the correction");
                    "<<<FILE index.html>>>\n<h1>ok</h1>\n<<<END>>>"
                };
                let reply = json!({"choices":[{"message":{"role":"assistant","content":content}}]});
                req.respond(
                    tiny_http::Response::from_string(reply.to_string())
                        .with_header(tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap()),
                )
                .unwrap();
            }
        });
        let agent = ApiAgent {
            url: format!("http://127.0.0.1:{port}/v1"),
            model: "m".into(),
            key: None,
            timeout: Duration::from_secs(10),
            max_total: Duration::from_secs(60),
            max_context_bytes: 1000,
        };
        let dir = std::env::temp_dir().join(format!("aysar-api-retry-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let out = agent.run(&dir, "Build the site").unwrap();
        assert!(out.ok, "{}", out.stderr_tail);
        assert!(out.stdout_tail.contains("after one correction"), "{}", out.stdout_tail);
        assert_eq!(out.files, vec!["index.html".to_string()]);
        handle.join().unwrap();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn reads_a_streamed_reply() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let handle = std::thread::spawn(move || {
            let req = server.recv().unwrap();
            let body =
                "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"<<<FILE a.txt>>>\\n\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"hello\\n\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"<<<END>>>\"}}]}\n\n\
data: [DONE]\n\n";
            req.respond(
                tiny_http::Response::from_string(body)
                    .with_header(tiny_http::Header::from_bytes("Content-Type", "text/event-stream").unwrap()),
            )
            .unwrap();
        });
        let agent = ApiAgent {
            url: format!("http://127.0.0.1:{port}/v1"),
            model: "m".into(),
            key: None,
            timeout: Duration::from_secs(10),
            max_total: Duration::from_secs(60),
            max_context_bytes: 1000,
        };
        let text = agent.chat(vec![json!({"role":"user","content":"hi"})]).unwrap();
        assert_eq!(text, "<<<FILE a.txt>>>\nhello\n<<<END>>>");
        handle.join().unwrap();
    }

    #[test]
    fn builds_through_a_mock_server() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let handle = std::thread::spawn(move || {
            for _ in 0..2 {
                let mut req = server.recv().unwrap();
                if req.url() == "/v1/models" {
                    req.respond(tiny_http::Response::from_string("{\"data\":[]}")).unwrap();
                    continue;
                }
                assert_eq!(req.url(), "/v1/chat/completions");
                let mut body = String::new();
                req.as_reader().read_to_string(&mut body).unwrap();
                assert!(body.contains("Aysar"));
                let reply = json!({"choices":[{"message":{"role":"assistant","content":"<<<FILE index.html>>>\n<html><title>T</title></html>\n<<<END>>>\n<<<FILE README.md>>>\nOpen index.html\n<<<END>>>"}}]});
                req.respond(
                    tiny_http::Response::from_string(reply.to_string())
                        .with_header(tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap()),
                )
                .unwrap();
            }
        });
        let agent = ApiAgent {
            url: format!("http://127.0.0.1:{port}/v1"),
            model: "m".into(),
            key: None,
            timeout: Duration::from_secs(10),
            max_total: Duration::from_secs(60),
            max_context_bytes: 1000,
        };
        assert!(agent.available());
        let dir = std::env::temp_dir().join(format!("aysar-api-build-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let out = agent.run(&dir, "Build the site in the Aysar spec below").unwrap();
        assert!(out.ok, "{}", out.stderr_tail);
        assert_eq!(out.files, vec!["README.md".to_string(), "index.html".to_string()]);
        handle.join().unwrap();
        let _ = fs::remove_dir_all(dir);
    }
}
