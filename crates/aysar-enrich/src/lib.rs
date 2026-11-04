//! Enrichment: ask a language model to add what rules cannot infer — the
//! purpose of each page, behaviours implied by the content, design notes —
//! and merge the answer into the spec as **inferences** with a confidence
//! below 1.0, so they can always be told apart from observation.
//!
//! Works with any OpenAI-compatible `/chat/completions` endpoint: Ollama,
//! llama.cpp, LM Studio, vLLM, OpenRouter, OpenAI. Nothing from the original
//! target other than the spec (which already excludes raw source) is sent.

use aysar_core::evidence::{EvidenceKind, Ledger};
use aysar_core::spec::{Behavior, Spec};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct LlmConfig {
    /// Base URL ending in `/v1`, e.g. `http://localhost:11434/v1`.
    pub url: String,
    pub model: String,
    pub key: Option<String>,
    pub timeout: Duration,
}

impl Default for LlmConfig {
    fn default() -> Self {
        LlmConfig {
            url: std::env::var("AYSAR_LLM_URL").unwrap_or_else(|_| "http://localhost:11434/v1".into()),
            model: std::env::var("AYSAR_LLM_MODEL").unwrap_or_else(|_| "llama3.1".into()),
            key: std::env::var("AYSAR_LLM_KEY").ok().filter(|k| !k.is_empty()),
            timeout: Duration::from_secs(180),
        }
    }
}

/// What we ask the model for. Everything is optional; unknown keys are ignored.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Enrichment {
    #[serde(default)]
    pub page_purposes: Vec<PagePurpose>,
    #[serde(default)]
    pub behaviors: Vec<SuggestedBehavior>,
    #[serde(default)]
    pub design_notes: Vec<String>,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PagePurpose {
    pub path: String,
    pub purpose: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SuggestedBehavior {
    pub id: String,
    pub description: String,
}

const SYSTEM: &str = "You are helping an engineer recreate a piece of software from a specification that was measured from the original. \
Return ONLY a JSON object with these optional keys: \
\"page_purposes\": [{\"path\": string, \"purpose\": string}], \
\"behaviors\": [{\"id\": kebab-case string, \"description\": one sentence a builder can implement and a tester can verify}], \
\"design_notes\": [string], \"summary\": string. \
Do not invent pages that are not in the spec. Do not suggest copying text, logos or images. Keep it short.";

/// Ask the model and parse its answer.
pub fn ask(cfg: &LlmConfig, spec: &Spec) -> anyhow::Result<Enrichment> {
    let body = serde_json::json!({
        "model": cfg.model,
        "temperature": 0.2,
        "messages": [
            {"role": "system", "content": SYSTEM},
            {"role": "user", "content": format!("Specification:\n\n{}", spec.to_markdown())}
        ]
    });
    let url = format!("{}/chat/completions", cfg.url.trim_end_matches('/'));
    let agent = ureq::AgentBuilder::new().timeout(cfg.timeout).build();
    let mut req = agent.post(&url).set("Content-Type", "application/json");
    if let Some(k) = &cfg.key {
        req = req.set("Authorization", &format!("Bearer {k}"));
    }
    let resp = req.send_json(body).map_err(|e| anyhow::anyhow!("{url}: {e}"))?;
    let v: serde_json::Value = resp.into_json()?;
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no message content in the model's reply"))?;
    parse_enrichment(content)
}

/// Pull the JSON object out of a reply that may be wrapped in prose or fences.
pub fn parse_enrichment(text: &str) -> anyhow::Result<Enrichment> {
    if let Ok(e) = serde_json::from_str::<Enrichment>(text) {
        return Ok(e);
    }
    let start = text.find('{').ok_or_else(|| anyhow::anyhow!("no JSON object in the model's reply"))?;
    let end = text.rfind('}').ok_or_else(|| anyhow::anyhow!("no JSON object in the model's reply"))?;
    Ok(serde_json::from_str(&text[start..=end])?)
}

/// Merge an enrichment into a spec. Returns how many items were added.
pub fn merge(spec: &mut Spec, e: &Enrichment, model: &str, ledger: Option<&Ledger>) -> anyhow::Result<usize> {
    let mut added = 0;
    let src = format!("llm:{model}");
    for pp in &e.page_purposes {
        if let Some(page) = spec.pages.iter_mut().find(|p| p.path == pp.path) {
            if page.purpose.is_none() {
                page.purpose = Some(pp.purpose.clone());
                added += 1;
                if let Some(l) = ledger {
                    l.record(
                        EvidenceKind::Inference,
                        format!("purpose of {}: {}", pp.path, pp.purpose),
                        &src,
                        0.6,
                        vec![],
                    )?;
                }
            }
        }
    }
    for b in &e.behaviors {
        if spec.behaviors.iter().any(|x| x.id == b.id) {
            continue;
        }
        let ev = match ledger {
            Some(l) => vec![l.record(EvidenceKind::Inference, b.description.clone(), &src, 0.6, vec![])?],
            None => vec![],
        };
        spec.behaviors.push(Behavior {
            id: b.id.clone(),
            description: format!("{} (suggested by {model})", b.description),
            evidence: ev,
        });
        added += 1;
    }
    for n in &e.design_notes {
        if !spec.design.notes.contains(n) {
            spec.design.notes.push(format!("{n} (suggested by {model})"));
            added += 1;
        }
    }
    if let Some(summary) = &e.summary {
        spec.limitations.retain(|l| !l.starts_with("Enrichment summary"));
        spec.limitations.push(format!("Enrichment summary ({model}): {summary}"));
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aysar_core::spec::{DesignSystem, PageSpec, Rules};
    use aysar_core::Kind;

    fn spec() -> Spec {
        Spec {
            spec_version: 1,
            name: "t".into(),
            kind: Kind::Website,
            request: "r".into(),
            source: "s".into(),
            mode: "clean-room".into(),
            generated_at: String::new(),
            stack: None,
            pages: vec![PageSpec { path: "/pricing".into(), ..Default::default() }],
            design: DesignSystem::default(),
            behaviors: vec![],
            rules: Rules::default(),
            program: None,
            limitations: vec![],
        }
    }

    #[test]
    fn parses_fenced_json() {
        let e = parse_enrichment("Sure!\n```json\n{\"page_purposes\":[{\"path\":\"/pricing\",\"purpose\":\"Plans and prices\"}],\"behaviors\":[{\"id\":\"toggle-billing\",\"description\":\"A monthly/yearly toggle updates every price.\"}]}\n```").unwrap();
        assert_eq!(e.page_purposes.len(), 1);
        let mut s = spec();
        let n = merge(&mut s, &e, "test-model", None).unwrap();
        assert_eq!(n, 2);
        assert_eq!(s.pages[0].purpose.as_deref(), Some("Plans and prices"));
        assert!(s.behaviors[0].description.contains("suggested by test-model"));
    }

    #[test]
    fn talks_to_an_openai_compatible_server() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let handle = std::thread::spawn(move || {
            let mut req = server.recv().unwrap();
            assert_eq!(req.url(), "/v1/chat/completions");
            assert!(req.headers().iter().any(|h| h.field.equiv("authorization") && h.value.as_str() == "Bearer k"));
            let mut body = String::new();
            req.as_reader().read_to_string(&mut body).unwrap();
            assert!(body.contains("\"model\":\"m\""));
            assert!(body.contains("/pricing"));
            let reply = serde_json::json!({"choices":[{"message":{"role":"assistant","content":"{\"design_notes\":[\"Keep prices in a 3-column grid\"],\"summary\":\"A pricing page.\"}"}}]});
            req.respond(
                tiny_http::Response::from_string(reply.to_string())
                    .with_header(tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap()),
            )
            .unwrap();
        });
        let cfg = LlmConfig {
            url: format!("http://127.0.0.1:{port}/v1"),
            model: "m".into(),
            key: Some("k".into()),
            timeout: Duration::from_secs(5),
        };
        let e = ask(&cfg, &spec()).unwrap();
        assert_eq!(e.design_notes.len(), 1);
        let mut s = spec();
        merge(&mut s, &e, "m", None).unwrap();
        assert!(s.design.notes[0].starts_with("Keep prices"));
        assert!(s.limitations[0].starts_with("Enrichment summary"));
        handle.join().unwrap();
    }
}
