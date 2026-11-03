# Spec format (v1)

The spec is the contract between study and build. It lives at `<project>/spec/spec.json`, with a human rendering at `spec/spec.md`. Edit the JSON; the Markdown is regenerated.

```jsonc
{
  "spec_version": 1,
  "name": "site",                 // project name
  "kind": "website",              // website | app | game | unknown
  "request": "Recreate this website in plain HTML",
  "source": "https://example.com",
  "mode": "clean-room",           // or "deep"
  "generated_at": "2026-11-03T14:01:41Z",
  "stack": "Plain HTML/CSS/JS",   // detected from the request, or null

  "pages": [
    {
      "path": "/",
      "title": "Nablus Bakery",
      "purpose": "Landing page: the first thing a visitor sees.",
      "headings": [ { "level": 1, "text": "Fresh bread, every morning" } ],
      "navigation": [ { "text": "Contact", "href": "/contact", "internal": true } ],
      "forms": [
        {
          "action": "/send", "method": "POST",
          "fields": [ { "name": "email", "input_type": "email", "placeholder": null, "required": true } ],
          "submit_label": "Send"
        }
      ],
      "landmarks": [ "header", "nav", "main", "footer" ],
      "text_sample": "first ~60 words, for tone and structure only",
      "word_count": 31,
      "image_count": 1
    }
  ],

  "design": {
    "colors": [ "#112233" ],      // Deep mode (or renderer) only
    "fonts": [ "Inter" ],
    "notes": [ "The original has a navigation landmark; keep a visible primary navigation." ]
  },

  "behaviors": [
    {
      "id": "form-contact-1",
      "description": "On `/contact`, a POST form to `/send` with fields … must validate required fields and give feedback.",
      "evidence": [ "ev-00003" ]   // ids in evidence/ledger.jsonl
    }
  ],

  "rules": {
    "no_asset_copying": true,
    "no_circumvention": true,
    "extra": [ "Use Arabic for all UI text" ]   // --rule on the CLI
  },

  "limitations": [ "Clean room: stylesheets and scripts were not read; …" ]
}
```

## Rules of the format

- **Additive versions.** New fields are optional; `spec_version` only bumps when a field changes meaning.
- **Paths are site-relative** (`/`, `/about`). The builder maps them to files (`about/index.html` or `about.html`); the checker normalises both.
- **`text_sample` is not content to copy.** It is there so the builder understands tone and structure. The rules say so, and the prompt repeats it.
- **`behaviors[].evidence`** points into the ledger. A behaviour with no evidence is a user decision or an enrichment, and is marked as such there.
- **Anything an AI adds** (planned `aysar enrich`) is written under the same fields but recorded as `Inference` evidence with confidence < 1.0, so it can be told apart from observation.

## Planned fields (phase 1+)

- `pages[].viewports`: per-width layout boxes and screenshot paths.
- `design.spacing`, `design.radius`, `design.type_scale`.
- `pages[].states`: hover/focus/open states.
- `screens` and `flows` for apps; `entities`, `inputs`, `rules` for games.
