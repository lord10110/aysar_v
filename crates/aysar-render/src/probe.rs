//! Behaviour probe: drive the rendered page a little and write down what
//! happened. Runs the same way on the original and on the build, so the two
//! results can be compared.
//!
//! - **Links**: every same-origin link is fetched (`HEAD`, falling back to
//!   `GET`) from inside the page; a link "works" if the response is 2xx/3xx.
//! - **Forms**: `checkValidity()` with empty fields (should be `false` when
//!   something is required) and again with plausible values (should be
//!   `true`). Nothing is submitted.
//! - **Hover / focus**: the mouse is moved over the first navigation link and
//!   the first button, and `focus()` is called on them; a change in colour,
//!   background, outline or text decoration counts as a state style.
//!
//! No form is submitted, no navigation is triggered, no login is attempted.

use aysar_core::capture::{BehaviorProbe, FormProbe};
use headless_chrome::Tab;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Default, Deserialize)]
struct LinkResult {
    href: String,
    ok: bool,
}

#[derive(Debug, Default, Deserialize)]
struct FormResult {
    required_fields: usize,
    blocks_empty_submit: bool,
    accepts_filled: bool,
    has_submit: bool,
}

#[derive(Debug, Default, Deserialize)]
struct Probed {
    #[serde(default)]
    links: Vec<LinkResult>,
    #[serde(default)]
    forms: Vec<FormResult>,
}

#[derive(Debug, Default, Deserialize)]
struct StateTarget {
    kind: String,
    x: f32,
    y: f32,
    #[serde(default)]
    index: usize,
}

/// Run the probe on the page currently loaded in `tab`.
pub fn probe(tab: &Arc<Tab>) -> anyhow::Result<BehaviorProbe> {
    let mut out = BehaviorProbe::default();

    // Links and forms, in one async evaluation.
    let v = tab.evaluate(PROBE_JS, true).map_err(|e| anyhow::anyhow!("probe: {e}"))?;
    let s = v.value.and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_else(|| "{}".into());
    let probed: Probed = serde_json::from_str(&s).unwrap_or_default();
    out.links = probed.links.into_iter().map(|l| (l.href, l.ok)).collect();
    out.forms = probed
        .forms
        .into_iter()
        .map(|f| FormProbe {
            required_fields: f.required_fields,
            blocks_empty_submit: f.blocks_empty_submit,
            accepts_filled: f.accepts_filled,
            has_submit: f.has_submit,
        })
        .collect();

    // Hover and focus: find targets, then act on each one for real.
    let v = tab.evaluate(STATE_TARGETS_JS, false).map_err(|e| anyhow::anyhow!("state targets: {e}"))?;
    let s = v.value.and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_else(|| "[]".into());
    let targets: Vec<StateTarget> = serde_json::from_str(&s).unwrap_or_default();

    for t in targets {
        let before = snapshot_style(tab, &t.kind, t.index)?;
        // Hover: a real mouse move through CDP, so :hover rules apply.
        let moved = tab
            .move_mouse_to_point(headless_chrome::browser::tab::point::Point { x: t.x as f64, y: t.y as f64 })
            .is_ok();
        std::thread::sleep(Duration::from_millis(120));
        if moved {
            let hover = snapshot_style(tab, &t.kind, t.index)?;
            if hover != before {
                out.hover_changes.push(t.kind.clone());
            }
            // Park the mouse away again.
            let _ = tab.move_mouse_to_point(headless_chrome::browser::tab::point::Point { x: 1.0, y: 1.0 });
            std::thread::sleep(Duration::from_millis(60));
        }
        // Focus.
        let js = format!(
            "(function(){{ const els = document.querySelectorAll({sel}); const el = els[{idx}]; if (!el) return 'none'; el.focus(); return document.activeElement === el ? 'focused' : 'not-focusable'; }})()",
            sel = serde_json::to_string(selector_for(&t.kind)).unwrap(),
            idx = t.index
        );
        let focused = tab.evaluate(&js, false).ok().and_then(|v| v.value).map(|v| v == "focused").unwrap_or(false);
        if focused {
            std::thread::sleep(Duration::from_millis(60));
            let focus = snapshot_style(tab, &t.kind, t.index)?;
            if focus != before {
                out.focus_changes.push(t.kind.clone());
            }
            let _ = tab.evaluate("document.activeElement && document.activeElement.blur()", false);
        }
    }
    Ok(out)
}

fn selector_for(kind: &str) -> &'static str {
    match kind {
        "nav a" => "nav a, header a",
        "button" => "button, input[type=submit], a.button, a.btn, [role=button]",
        "input" => "input:not([type=hidden]):not([type=submit]), textarea",
        _ => "a",
    }
}

fn snapshot_style(tab: &Arc<Tab>, kind: &str, index: usize) -> anyhow::Result<String> {
    let js = format!(
        "(function(){{ const els = document.querySelectorAll({sel}); const el = els[{idx}]; if (!el) return ''; const cs = getComputedStyle(el); return [cs.color, cs.backgroundColor, cs.outlineStyle, cs.outlineColor, cs.outlineWidth, cs.textDecorationLine, cs.borderColor, cs.boxShadow, cs.opacity, cs.transform].join('|'); }})()",
        sel = serde_json::to_string(selector_for(kind)).unwrap(),
        idx = index
    );
    let v = tab.evaluate(&js, false).map_err(|e| anyhow::anyhow!("style: {e}"))?;
    Ok(v.value.and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default())
}

/// Async: returns a Promise that resolves to a JSON string.
const PROBE_JS: &str = r#"
(async function () {
  const origin = location.origin;
  const seen = new Set();
  const links = [];
  for (const a of document.querySelectorAll('a[href]')) {
    let u; try { u = new URL(a.getAttribute('href'), location.href); } catch (e) { continue; }
    if (u.origin !== origin) continue;
    if (u.protocol !== 'http:' && u.protocol !== 'https:') continue;
    u.hash = '';
    const key = u.pathname + u.search;
    if (seen.has(key)) continue;
    seen.add(key);
    if (links.length >= 25) break;
    if (links.length) await new Promise(r => setTimeout(r, 150));   // polite pacing
    let ok = false;
    try {
      let r = await fetch(u.href, { method: 'HEAD', redirect: 'follow' });
      if (r.status === 405 || r.status === 501) r = await fetch(u.href, { method: 'GET', redirect: 'follow' });
      ok = r.ok || (r.status >= 300 && r.status < 400);
    } catch (e) { ok = false; }
    links.push({ href: key, ok });
  }

  const forms = [];
  for (const f of document.querySelectorAll('form')) {
    const fields = [...f.querySelectorAll('input, textarea, select')].filter(x => !['hidden','submit','button','reset'].includes(x.type));
    const required = fields.filter(x => x.required).length;
    const has_submit = !!f.querySelector('button:not([type=button]):not([type=reset]), input[type=submit]');
    const saved = fields.map(x => ({ el: x, value: x.value, checked: x.checked }));
    for (const x of fields) { if (x.type === 'checkbox' || x.type === 'radio') x.checked = false; else if (x.tagName !== 'SELECT') x.value = ''; }
    const blocks_empty_submit = required > 0 ? !f.checkValidity() : true;
    for (const x of fields) {
      const t = (x.type || x.tagName).toLowerCase();
      if (t === 'checkbox' || t === 'radio') x.checked = true;
      else if (t === 'email') x.value = 'probe@example.com';
      else if (t === 'number' || t === 'range') x.value = x.min || '1';
      else if (t === 'tel') x.value = '+10000000000';
      else if (t === 'url') x.value = 'https://example.com';
      else if (t === 'date') x.value = '2026-01-01';
      else if (t === 'select') { if (x.options.length) x.selectedIndex = x.options.length - 1; }
      else if (t === 'file') {}
      else x.value = 'Probe text';
    }
    const accepts_filled = f.checkValidity();
    for (const s of saved) { s.el.value = s.value; s.el.checked = s.checked; }
    forms.push({ required_fields: required, blocks_empty_submit, accepts_filled, has_submit });
  }
  return JSON.stringify({ links, forms });
})()
"#;

/// Sync: centre points of the first nav link, first button, first input.
const STATE_TARGETS_JS: &str = r#"
(function () {
  const pick = (kind, sel) => {
    const els = document.querySelectorAll(sel);
    for (let i = 0; i < els.length; i++) {
      const r = els[i].getBoundingClientRect();
      if (r.width > 2 && r.height > 2 && r.top >= 0 && r.top < innerHeight) {
        return { kind, x: r.left + r.width / 2, y: r.top + r.height / 2, index: i };
      }
    }
    return null;
  };
  const out = [
    pick('nav a', 'nav a, header a'),
    pick('button', 'button, input[type=submit], a.button, a.btn, [role=button]'),
    pick('input', 'input:not([type=hidden]):not([type=submit]), textarea'),
  ].filter(Boolean);
  return JSON.stringify(out);
})()
"#;
