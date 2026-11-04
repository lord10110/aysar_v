//! Black-box tests of the `aysar` binary. They never need a coding agent
//! (dry-run) and turn rendering off so they run anywhere in seconds.

use std::path::PathBuf;
use std::process::Command;

fn aysar() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_aysar"));
    c.env("AYSAR_NO_RENDER", "1").env("AYSAR_CONFIG", "/nonexistent/aysar.toml");
    c
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aysar-cli-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sample_site(dir: &std::path::Path) -> PathBuf {
    let site = dir.join("site");
    std::fs::create_dir_all(site.join("about")).unwrap();
    std::fs::write(site.join("index.html"), "<html><head><title>Demo</title></head><body><header><nav><a href=\"/about\">About</a></nav></header><main><h1>Hello</h1><p>one two three</p></main><footer>f</footer></body></html>").unwrap();
    std::fs::write(site.join("about").join("index.html"), "<html><head><title>About</title></head><body><main><h1>About</h1><form method=\"post\"><input name=\"email\" type=\"email\" required><button type=\"submit\">Go</button></form></main></body></html>").unwrap();
    site
}

#[test]
fn doctor_runs() {
    let out = aysar().arg("doctor").output().unwrap();
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("Coding agents"));
    assert!(s.contains("dry-run"));
}

#[test]
fn rebuild_dry_run_then_check_and_status_json() {
    let dir = tmp("rebuild");
    let site = sample_site(&dir);
    let projects = dir.join("projects");

    let out = aysar()
        .args([
            "rebuild",
            site.to_str().unwrap(),
            "Recreate this website",
            "--agent",
            "dry-run",
            "--kind",
            "website",
            "--projects-dir",
        ])
        .arg(&projects)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let project = projects.join("site");
    assert!(project.join("aysar.json").exists());
    assert!(project.join("spec").join("spec.md").exists());
    assert!(project.join("app").join("BUILD-PROMPT.md").exists());
    let spec = std::fs::read_to_string(project.join("spec").join("spec.md")).unwrap();
    assert!(spec.contains("### `/about`"));
    assert!(spec.contains("email:email"));

    // Pretend an agent built a perfect copy.
    std::fs::create_dir_all(project.join("app").join("about")).unwrap();
    std::fs::copy(site.join("index.html"), project.join("app").join("index.html")).unwrap();
    std::fs::copy(site.join("about").join("index.html"), project.join("app").join("about").join("index.html")).unwrap();

    let out = aysar().args(["check", "--json"]).arg(&project).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["score"].as_f64().unwrap() > 99.0, "{v}");
    assert_eq!(v["mismatches"].as_array().unwrap().len(), 0);

    let out = aysar().args(["status", "--json"]).arg(&project).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["stage"], "checked");
    assert_eq!(v["kind"], "website");

    let out = aysar().args(["evidence", "--kind", "check"]).arg(&project).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("check 1"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn study_and_compare_without_project() {
    let dir = tmp("study");
    let site = sample_site(&dir);
    let out = aysar().args(["study"]).arg(&site).args(["--out"]).arg(dir.join("cap.json")).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("2 page(s)"));
    assert!(dir.join("cap.json").exists());

    let out = aysar().args(["compare"]).arg(&site).arg(dir.join("cap.json")).output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("# Check — 100/100"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn identify_setup_completions_config() {
    let dir = tmp("misc");
    let f = dir.join("x.html");
    std::fs::write(&f, "<!doctype html>").unwrap();
    let out = aysar().arg("identify").arg(&f).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("HTML document"));

    let out = aysar().arg("setup").output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("mcpServers"));

    let out = aysar().args(["completions", "bash"]).output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("_aysar"));

    let cfg = dir.join("config.toml");
    let out = Command::new(env!("CARGO_BIN_EXE_aysar"))
        .env("AYSAR_CONFIG", &cfg)
        .args(["config", "--init"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(cfg.exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn mcp_round_trip() {
    use std::io::Write;
    let mut child =
        aysar().arg("mcp").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{}}}}"#).unwrap();
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("\"name\":\"aysar\""));
    assert!(text.contains("aysar_rebuild"));
}

#[test]
fn bad_agent_is_an_error() {
    let dir = tmp("badagent");
    let site = sample_site(&dir);
    let out = aysar()
        .args(["rebuild", site.to_str().unwrap(), "x", "--agent", "no-such-agent", "--projects-dir"])
        .arg(dir.join("p"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown agent"));
    let _ = std::fs::remove_dir_all(dir);
}
