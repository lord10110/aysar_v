//! A tiny static file server, so a local folder (a captured site or a
//! generated build) can be rendered with real `/path` links. Listens on
//! 127.0.0.1 on a free port; stops when dropped.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use tiny_http::{Header, Response, Server};

pub struct StaticServer {
    port: u16,
    stop: Arc<AtomicBool>,
    server: Arc<Server>,
    thread: Option<JoinHandle<()>>,
}

impl StaticServer {
    /// Serve `root` on a free local port.
    pub fn serve(root: &Path) -> anyhow::Result<StaticServer> {
        let root = root.canonicalize()?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let server = Arc::new(Server::from_listener(listener, None).map_err(|e| anyhow::anyhow!("{e}"))?);
        let stop = Arc::new(AtomicBool::new(false));

        let (s2, stop2) = (server.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            for req in s2.incoming_requests() {
                if stop2.load(Ordering::Relaxed) {
                    break;
                }
                let url = req.url().split(['?', '#']).next().unwrap_or("/").to_string();
                match resolve(&root, &url) {
                    Some(file) => {
                        let mime = mime_for(&file);
                        match std::fs::read(&file) {
                            Ok(bytes) => {
                                let resp = Response::from_data(bytes)
                                    .with_header(Header::from_bytes("Content-Type", mime).unwrap());
                                let _ = req.respond(resp);
                            }
                            Err(_) => {
                                let _ = req.respond(Response::from_string("500").with_status_code(500));
                            }
                        }
                    }
                    None => {
                        let _ = req.respond(Response::from_string("404 not found").with_status_code(404));
                    }
                }
            }
        });

        Ok(StaticServer { port, stop, server, thread: Some(thread) })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Base URL without a trailing slash, e.g. `http://127.0.0.1:43210`.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for StaticServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.server.unblock();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Map a URL path to a file under `root`, the way a static host would:
/// `/` → `index.html`; `/about` → `about/index.html` or `about.html`.
/// Never escapes `root`.
pub fn resolve(root: &Path, url_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(url_path.trim_start_matches('/'));
    if decoded.contains("..") {
        return None;
    }
    let candidates: Vec<PathBuf> = if decoded.is_empty() {
        vec![root.join("index.html")]
    } else {
        let p = root.join(&decoded);
        vec![p.clone(), p.join("index.html"), root.join(format!("{}.html", decoded.trim_end_matches('/')))]
    };
    for c in candidates {
        if c.is_file() {
            if let Ok(canon) = c.canonicalize() {
                if canon.starts_with(root) {
                    return Some(canon);
                }
            }
        }
    }
    None
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

fn mime_for(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_like_a_static_host() {
        let dir = std::env::temp_dir().join(format!("aysar-srv-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("about")).unwrap();
        std::fs::write(dir.join("index.html"), "home").unwrap();
        std::fs::write(dir.join("about").join("index.html"), "about").unwrap();
        std::fs::write(dir.join("contact.html"), "contact").unwrap();
        let root = dir.canonicalize().unwrap();
        assert!(resolve(&root, "/").unwrap().ends_with("index.html"));
        assert!(resolve(&root, "/about").unwrap().ends_with("about/index.html"));
        assert!(resolve(&root, "/about/").unwrap().ends_with("about/index.html"));
        assert!(resolve(&root, "/contact").unwrap().ends_with("contact.html"));
        assert!(resolve(&root, "/../etc/passwd").is_none());
        assert!(resolve(&root, "/missing").is_none());

        let s = StaticServer::serve(&dir).unwrap();
        let body = std::net::TcpStream::connect(("127.0.0.1", s.port())).map(|mut st| {
            use std::io::{Read, Write};
            st.write_all(b"GET /about HTTP/1.0\r\n\r\n").unwrap();
            let mut out = String::new();
            st.read_to_string(&mut out).unwrap();
            out
        });
        assert!(body.unwrap().ends_with("about"));
        drop(s);
        let _ = std::fs::remove_dir_all(dir);
    }
}
