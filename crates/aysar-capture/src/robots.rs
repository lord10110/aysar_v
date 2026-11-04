//! A small robots.txt reader: `Disallow`/`Allow` for `*` and for our own
//! user-agent, plus `Crawl-delay`. Longest matching rule wins, as in the
//! standard; `Allow` wins a tie.

#[derive(Debug, Default, Clone)]
pub struct Rules {
    allow: Vec<String>,
    disallow: Vec<String>,
    crawl_delay_secs: Option<f32>,
}

impl Rules {
    /// Parse robots.txt for `agent` (case-insensitive), falling back to `*`.
    pub fn parse(text: &str, agent: &str) -> Rules {
        let agent = agent.to_ascii_lowercase();
        let mut groups: Vec<(Vec<String>, Rules)> = Vec::new();
        let mut cur_agents: Vec<String> = Vec::new();
        let mut cur = Rules::default();
        let mut in_rules = false;

        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let Some((k, v)) = line.split_once(':') else { continue };
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim().to_string();
            match k.as_str() {
                "user-agent" => {
                    if in_rules {
                        groups.push((std::mem::take(&mut cur_agents), std::mem::take(&mut cur)));
                        in_rules = false;
                    }
                    cur_agents.push(v.to_ascii_lowercase());
                }
                "disallow" => {
                    in_rules = true;
                    if !v.is_empty() {
                        cur.disallow.push(v);
                    }
                }
                "allow" => {
                    in_rules = true;
                    if !v.is_empty() {
                        cur.allow.push(v);
                    }
                }
                "crawl-delay" => {
                    in_rules = true;
                    cur.crawl_delay_secs = v.parse().ok();
                }
                _ => {}
            }
        }
        if !cur_agents.is_empty() {
            groups.push((cur_agents, cur));
        }
        // Our own group wins over `*`.
        if let Some((_, r)) =
            groups.iter().find(|(a, _)| a.iter().any(|x| x == &agent || (x != "*" && agent.starts_with(x.as_str()))))
        {
            return r.clone();
        }
        groups.into_iter().find(|(a, _)| a.iter().any(|x| x == "*")).map(|(_, r)| r).unwrap_or_default()
    }

    pub fn allows(&self, path: &str) -> bool {
        let best_allow = self.allow.iter().filter(|p| matches(p, path)).map(|p| p.len()).max();
        let best_disallow = self.disallow.iter().filter(|p| matches(p, path)).map(|p| p.len()).max();
        match (best_allow, best_disallow) {
            (_, None) => true,
            (None, Some(_)) => false,
            (Some(a), Some(d)) => a >= d,
        }
    }

    pub fn crawl_delay_ms(&self) -> u64 {
        self.crawl_delay_secs.map(|s| (s.clamp(0.0, 30.0) * 1000.0) as u64).unwrap_or(0)
    }
}

/// `*` matches anything; `$` anchors the end.
fn matches(pattern: &str, path: &str) -> bool {
    let anchored = pattern.ends_with('$');
    let pat = pattern.trim_end_matches('$');
    let parts: Vec<&str> = pat.split('*').collect();
    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let found = if i == 0 {
            if path.starts_with(part) {
                Some(0)
            } else {
                None
            }
        } else {
            path[pos..].find(part).map(|f| pos + f)
        };
        match found {
            Some(f) => pos = f + part.len(),
            None => return false,
        }
    }
    if anchored {
        if parts.last().map(|p| p.is_empty()).unwrap_or(true) {
            true
        } else {
            pos == path.len()
        }
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules() {
        let r = Rules::parse("User-agent: *\nDisallow: /private/\nAllow: /private/ok\nDisallow: /*.pdf$\nCrawl-delay: 2\n\nUser-agent: aysar\nDisallow: /secret\n", "aysar/0.1");
        // Our own group applies.
        assert!(!r.allows("/secret/x"));
        assert!(r.allows("/private/"));

        let g = Rules::parse(
            "User-agent: *\nDisallow: /private/\nAllow: /private/ok\nDisallow: /*.pdf$\nCrawl-delay: 2\n",
            "aysar",
        );
        assert!(g.allows("/"));
        assert!(!g.allows("/private/a"));
        assert!(g.allows("/private/ok/page"));
        assert!(!g.allows("/docs/file.pdf"));
        assert!(g.allows("/docs/file.pdfx"));
        assert_eq!(g.crawl_delay_ms(), 2000);
        assert!(Rules::default().allows("/anything"));
    }
}
