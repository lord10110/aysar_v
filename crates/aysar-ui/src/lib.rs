//! `aysar ui`: a dashboard on 127.0.0.1 for the projects folder.
//!
//! One HTML page (embedded, no build step) talks to a small JSON API:
//!
//! - `GET  /api/projects`                      list
//! - `GET  /api/projects/<name>`               state + spec + checks + evidence
//! - `GET  /api/projects/<name>/file/<path>`   a file inside the project (pictures, reports)
//! - `POST /api/projects/<name>/check`         re-check `app/`
//! - `POST /api/projects/<name>/build`         one more build round, then check
//! - `POST /api/rebuild`  {target, request, kind, agent, deep}   new project
//! - `GET  /api/jobs`                          background jobs and their logs
//! - `GET  /api/doctor`
//!
//! Jobs run on a thread; the page polls `/api/jobs`. Nothing listens on
//! anything but loopback, and there is no authentication, so this is for the
//! machine it runs on.

use aysar_core::evidence::Ledger;
use aysar_core::project::Project;
use aysar_core::{Kind, Mode, Target};
use aysar_orchestrator::{RebuildOptions, Reporter};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Method, Request, Response, Server};

pub const PAGE: &str = include_str!("ui.html");

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: usize,
    pub title: String,
    pub project: Option<String>,
    pub done: bool,
    pub ok: Option<bool>,
    pub log: Vec<String>,
    pub started_at: String,
}

#[derive(Default)]
pub struct Jobs {
    pub items: Vec<Job>,
}

struct JobReporter {
    jobs: Arc<Mutex<Jobs>>,
    id: usize,
}

impl Reporter for JobReporter {
    fn say(&self, msg: &str) {
        if let Ok(mut j) = self.jobs.lock() {
            if let Some(job) = j.items.iter_mut().find(|x| x.id == self.id) {
                for line in msg.lines() {
                    job.log.push(line.to_string());
                }
                if job.log.len() > 400 {
                    let drop = job.log.len() - 400;
                    job.log.drain(0..drop);
                }
            }
        }
    }
}

pub struct Ui {
    pub projects_dir: PathBuf,
    jobs: Arc<Mutex<Jobs>>,
}

impl Ui {
    pub fn new(projects_dir: PathBuf) -> Ui {
        Ui { projects_dir, jobs: Arc::new(Mutex::new(Jobs::default())) }
    }

    /// Serve forever on `port` (0 = any free port). Returns the bound port
    /// through `on_ready` before blocking.
    pub fn serve(self, port: u16, on_ready: impl FnOnce(u16)) -> anyhow::Result<()> {
        let server = Server::http(("127.0.0.1", port)).map_err(|e| anyhow::anyhow!("{e}"))?;
        let bound = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(port);
        on_ready(bound);
        let ui = Arc::new(self);
        for req in server.incoming_requests() {
            let ui = ui.clone();
            // Each request on its own thread so a slow file never blocks the page.
            std::thread::spawn(move || {
                let _ = ui.handle(req);
            });
        }
        Ok(())
    }

    fn handle(&self, mut req: Request) -> anyhow::Result<()> {
        let url = req.url().to_string();
        let path = url.split('?').next().unwrap_or("/").to_string();
        let method = req.method().clone();
        let mut body = String::new();
        if method == Method::Post {
            let _ = req.as_reader().read_to_string(&mut body);
        }
        let resp = match (method, path.as_str()) {
            (Method::Get, "/") | (Method::Get, "/index.html") => html(PAGE),
            (Method::Get, "/api/doctor") => json_resp(self.doctor()),
            (Method::Get, "/api/projects") => json_resp(self.list()),
            (Method::Get, "/api/jobs") => json_resp(self.jobs_json()),
            (Method::Post, "/api/rebuild") => match self.start_rebuild(&body) {
                Ok(v) => json_resp(v),
                Err(e) => error(400, &e.to_string()),
            },
            (m, p) if p.starts_with("/api/projects/") => {
                let rest = &p["/api/projects/".len()..];
                let (name, sub) = match rest.find('/') {
                    Some(i) => (&rest[..i], &rest[i..]),
                    None => (rest, ""),
                };
                let name = percent_decode(name);
                if name.contains("..") || name.contains('/') {
                    error(400, "bad project name")
                } else {
                    match (m, sub) {
                        (Method::Get, "") => match self.project(&name) {
                            Ok(v) => json_resp(v),
                            Err(e) => error(404, &e.to_string()),
                        },
                        (Method::Get, s) if s.starts_with("/file/") => {
                            self.file(&name, &percent_decode(&s["/file/".len()..]))
                        }
                        (Method::Post, "/check") => json_resp(self.start_check(&name)),
                        (Method::Post, "/build") => json_resp(self.start_build(&name, &body)),
                        _ => error(404, "not found"),
                    }
                }
            }
            _ => error(404, "not found"),
        };
        req.respond(resp)?;
        Ok(())
    }

    fn doctor(&self) -> Value {
        let agents: Vec<Value> = aysar_build::status_detailed()
            .into_iter()
            .map(|a| json!({"name": a.name, "installed": a.installed, "verified": a.verified, "command": a.command}))
            .collect();
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "projects_dir": self.projects_dir.display().to_string(),
            "chrome": aysar_render::find_chrome().map(|p| p.display().to_string()),
            "agents": agents,
        })
    }

    fn list(&self) -> Value {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.projects_dir) {
            for e in rd.flatten() {
                let p = e.path();
                if Project::exists(&p) {
                    if let Ok(pr) = Project::open(&p) {
                        let s = &pr.state;
                        out.push(json!({
                            "name": s.name, "request": s.request, "target": s.target.to_string(),
                            "kind": s.kind.to_string(), "mode": s.mode.to_string(), "stage": s.stage,
                            "score": s.score, "agent": s.agent, "updated_at": s.updated_at,
                        }));
                    }
                }
            }
        }
        out.sort_by(|a, b| b["updated_at"].as_str().cmp(&a["updated_at"].as_str()));
        json!({"projects": out})
    }

    fn project(&self, name: &str) -> anyhow::Result<Value> {
        let root = self.projects_dir.join(name);
        let p = Project::open(&root)?;
        let spec_md = std::fs::read_to_string(p.spec_markdown()).unwrap_or_default();
        let mut checks = Vec::new();
        if let Ok(rd) = std::fs::read_dir(p.checks_dir()) {
            let mut files: Vec<PathBuf> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|x| x.extension().map(|e| e == "json").unwrap_or(false))
                .collect();
            files.sort();
            for f in files {
                if let Ok(text) = std::fs::read_to_string(&f) {
                    if let Ok(mut v) = serde_json::from_str::<Value>(&text) {
                        let stem = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                        let shots = p.checks_dir().join(format!("{stem}-screenshots"));
                        let mut pictures: Vec<String> = Vec::new();
                        if let Ok(rd) = std::fs::read_dir(&shots) {
                            for e in rd.flatten() {
                                let n = e.file_name().to_string_lossy().to_string();
                                if n.ends_with("-diff.png") {
                                    pictures.push(format!("checks/{stem}-screenshots/{n}"));
                                }
                            }
                        }
                        pictures.sort();
                        v["id"] = json!(stem);
                        v["pictures"] = json!(pictures);
                        v["markdown"] = json!(std::fs::read_to_string(f.with_extension("md")).unwrap_or_default());
                        checks.push(v);
                    }
                }
            }
        }
        let mut screenshots: Vec<String> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(p.capture_dir().join("screenshots")) {
            for e in rd.flatten() {
                screenshots.push(format!("capture/screenshots/{}", e.file_name().to_string_lossy()));
            }
        }
        screenshots.sort();
        let evidence: Vec<Value> = Ledger::open(p.ledger_file())
            .and_then(|l| l.all())
            .unwrap_or_default()
            .into_iter()
            .rev()
            .take(200)
            .map(|e| serde_json::to_value(e).unwrap_or(Value::Null))
            .collect();
        let mut inbox = Vec::new();
        if let Ok(rd) = std::fs::read_dir(p.inbox_dir()) {
            for e in rd.flatten() {
                inbox.push(json!({"file": e.file_name().to_string_lossy(), "text": std::fs::read_to_string(e.path()).unwrap_or_default()}));
            }
        }
        Ok(json!({
            "state": p.state,
            "root": p.root.display().to_string(),
            "app_dir": p.app_dir().display().to_string(),
            "spec_markdown": spec_md,
            "checks": checks,
            "screenshots": screenshots,
            "evidence": evidence,
            "inbox": inbox,
        }))
    }

    fn file(&self, name: &str, rel: &str) -> Response<std::io::Cursor<Vec<u8>>> {
        if rel.contains("..") {
            return error(400, "bad path");
        }
        let root = self.projects_dir.join(name);
        let path = root.join(rel);
        let Ok(canon) = path.canonicalize() else { return error(404, "no such file") };
        let Ok(root_c) = root.canonicalize() else { return error(404, "no such project") };
        if !canon.starts_with(&root_c) || !canon.is_file() {
            return error(404, "no such file");
        }
        match std::fs::read(&canon) {
            Ok(bytes) => {
                let mime = match canon.extension().and_then(|e| e.to_str()).unwrap_or("") {
                    "png" => "image/png",
                    "md" | "txt" => "text/plain; charset=utf-8",
                    "json" | "jsonl" => "application/json",
                    "html" => "text/plain; charset=utf-8", // never execute a build's HTML on this origin
                    _ => "application/octet-stream",
                };
                Response::from_data(bytes).with_header(Header::from_bytes("Content-Type", mime).unwrap())
            }
            Err(_) => error(404, "no such file"),
        }
    }

    fn jobs_json(&self) -> Value {
        let j = self.jobs.lock().map(|j| j.items.clone()).unwrap_or_default();
        json!({"jobs": j})
    }

    fn new_job(&self, title: &str, project: Option<String>) -> usize {
        let mut j = self.jobs.lock().unwrap();
        let id = j.items.len() + 1;
        j.items.push(Job {
            id,
            title: title.into(),
            project,
            done: false,
            ok: None,
            log: vec![],
            started_at: aysar_core::now_rfc3339(),
        });
        id
    }

    fn finish(jobs: &Arc<Mutex<Jobs>>, id: usize, ok: bool, msg: Option<String>) {
        if let Ok(mut j) = jobs.lock() {
            if let Some(job) = j.items.iter_mut().find(|x| x.id == id) {
                job.done = true;
                job.ok = Some(ok);
                if let Some(m) = msg {
                    job.log.push(m);
                }
            }
        }
    }

    fn start_check(&self, name: &str) -> Value {
        let id = self.new_job(&format!("Check {name}"), Some(name.into()));
        let root = self.projects_dir.join(name);
        let jobs = self.jobs.clone();
        std::thread::spawn(move || {
            let rep = JobReporter { jobs: jobs.clone(), id };
            let r = Project::open(&root).and_then(|mut p| aysar_orchestrator::check(&mut p, &rep).map(|_| ()));
            Self::finish(&jobs, id, r.is_ok(), r.err().map(|e| format!("error: {e:#}")));
        });
        json!({"job": id})
    }

    fn start_build(&self, name: &str, body: &str) -> Value {
        let agent = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| v["agent"].as_str().map(|s| s.to_string()))
            .filter(|s| !s.is_empty());
        let id = self.new_job(&format!("Build {name}"), Some(name.into()));
        let root = self.projects_dir.join(name);
        let jobs = self.jobs.clone();
        std::thread::spawn(move || {
            let rep = JobReporter { jobs: jobs.clone(), id };
            let r = (|| -> anyhow::Result<()> {
                let mut p = Project::open(&root)?;
                let round = p.next_check_number();
                let prev = aysar_orchestrator::latest_report(&p)?;
                let agent = agent.or_else(|| p.state.agent.clone());
                let out = aysar_orchestrator::build(&mut p, agent.as_deref(), round, prev.as_ref(), &rep)?;
                if out.agent != "dry-run" {
                    aysar_orchestrator::check(&mut p, &rep)?;
                }
                Ok(())
            })();
            Self::finish(&jobs, id, r.is_ok(), r.err().map(|e| format!("error: {e:#}")));
        });
        json!({"job": id})
    }

    fn start_rebuild(&self, body: &str) -> anyhow::Result<Value> {
        let v: Value = serde_json::from_str(body)?;
        let target = Target::parse(v["target"].as_str().unwrap_or("-"));
        let request = v["request"].as_str().unwrap_or("").trim().to_string();
        if request.is_empty() {
            anyhow::bail!("say what you want to make");
        }
        let kind = match v["kind"].as_str().unwrap_or("") {
            "website" => Some(Kind::Website),
            "app" => Some(Kind::App),
            "game" => Some(Kind::Game),
            _ => None,
        };
        let mode = if v["deep"].as_bool().unwrap_or(false) { Mode::Deep } else { Mode::CleanRoom };
        let agent = v["agent"].as_str().map(|s| s.to_string()).filter(|s| !s.is_empty());
        let mut p = aysar_orchestrator::new_project(&self.projects_dir, target, &request, mode, kind)?;
        let name = p.state.name.clone();
        let id = self.new_job(&format!("Rebuild {name}"), Some(name.clone()));
        let jobs = self.jobs.clone();
        let cfg = aysar_orchestrator::config::Config::load();
        let opts = RebuildOptions {
            mode,
            agent: agent.or(cfg.agent),
            rounds: cfg.rounds.unwrap_or(2),
            target_score: cfg.target_score.unwrap_or(85.0),
            max_pages: cfg.max_pages.unwrap_or(5),
            ..Default::default()
        };
        std::thread::spawn(move || {
            let rep = JobReporter { jobs: jobs.clone(), id };
            let r = aysar_orchestrator::rebuild(&mut p, &opts, &rep);
            Self::finish(&jobs, id, r.is_ok(), r.err().map(|e| format!("error: {e:#}")));
        });
        Ok(json!({"job": id, "project": name}))
    }
}

fn html(s: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(s).with_header(Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap())
}

fn json_resp(v: Value) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(v.to_string()).with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
}

fn error(code: u16, msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    json_resp(json!({"error": msg})).with_status_code(code)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[allow(dead_code)]
fn _p(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn get(port: u16, path: &str) -> (u16, String) {
        let mut st = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(st, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").unwrap();
        let mut s = String::new();
        st.read_to_string(&mut s).unwrap();
        let code: u16 = s.split_whitespace().nth(1).unwrap().parse().unwrap();
        let body = s.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        (code, body)
    }

    #[test]
    fn api_lists_and_reads_projects() {
        let dir = std::env::temp_dir().join(format!("aysar-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut p = Project::create(dir.join("demo"), "demo", "make it", Target::parse("-"), Mode::CleanRoom).unwrap();
        p.state.score = Some(42.0);
        p.save().unwrap();
        std::fs::write(p.spec_markdown(), "# Spec: demo").unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        let d2 = dir.clone();
        std::thread::spawn(move || {
            Ui::new(d2).serve(0, move |port| tx.send(port).unwrap()).unwrap();
        });
        let port = rx.recv().unwrap();

        let (code, body) = get(port, "/");
        assert_eq!(code, 200);
        assert!(body.contains("<title>Aysar</title>"));
        let (code, body) = get(port, "/api/projects");
        assert_eq!(code, 200);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["projects"][0]["name"], "demo");
        assert_eq!(v["projects"][0]["score"], 42.0);
        let (code, body) = get(port, "/api/projects/demo");
        assert_eq!(code, 200);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["spec_markdown"], "# Spec: demo");
        let (code, _) = get(port, "/api/projects/demo/file/../../etc/passwd");
        assert_eq!(code, 400);
        let (code, _) = get(port, "/api/projects/nope");
        assert_eq!(code, 404);
        let (code, body) = get(port, "/api/doctor");
        assert_eq!(code, 200);
        assert!(body.contains("\"agents\""));
        let _ = std::fs::remove_dir_all(dir);
    }
}
