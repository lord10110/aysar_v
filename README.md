# Aysar

**English** · [العربية](README.ar.md)

**See how any website, app or game works, then have a coding agent build your own — from evidence, through a reviewable spec, checked against the original.**

Aysar is an open-source engine in pure Rust. Point it at a target, say what you want, and it:

1. **Studies** the original with real inspection tools: structure, text, forms, and — in headless Chromium — screenshots at 360/768/1280px, measured colours, fonts, type scale, spacing and where every region sits (Clean room by default: only what a visitor sees).
2. **Writes a spec** — a readable, editable intermediate specification. You can review and change it before a single line of code is written.
3. **Builds** a new project with the coding agent you already have (Claude Code, Codex, or any command-line agent).
4. **Checks** the build against the original — structure, content shape, and rendered visuals (screenshot hash, region layout, page height, palette and fonts at each width) — and lists what doesn't match yet, in plain sentences.
5. **Refines** until the score reaches your target.

Every claim Aysar makes is recorded in an evidence ledger with its source and confidence.

[![Rust](https://img.shields.io/badge/100%25-Rust-b7410e?style=flat-square&logo=rust)](#)
[![License](https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-3a3a3a?style=flat-square)](#license)
[![Status](https://img.shields.io/badge/status-alpha-d69e2e?style=flat-square)](docs/ROADMAP.md)

## Why Aysar

Tools like [Craft Anything](https://github.com/storytold/craft-anything) proved the idea: study, build, check. Aysar builds on that idea and on that engine where it helps, and differs in five places:

| | Aysar |
|---|---|
| **Spec in the middle** | Study → **Spec** → Build. The spec is a file you can read, diff and edit. The builder works from the spec, not from raw tool dumps. |
| **Agent-agnostic** | One `Agent` trait. Claude Code and Codex are built in; any other agent is one command template away (`aider --message-file {prompt_file}`). Dry-run mode works with no AI at all. |
| **Checked, with reasons** | Every mismatch is an actionable sentence (`/contact: form is missing fields email:email`, `/ at 360/768/1280px: <footer> is 66px tall in the original and 18px in the build`). Checks feed straight back into the next build round. |
| **Evidence ledger** | Observations, inferences, checks and decisions, each with a source and confidence, queryable (`aysar evidence`). |
| **Engines, not rewrites** | Disassembly and decompilation are delegated to existing engines (craft-anything first) behind one `Engine` trait. Aysar's own code is the orchestration, the spec, the verification and the gateway. |

## Quick start

Requires Rust 1.80+. For screenshots, measured styles and visual checks, a Chrome or Chromium on the machine (found automatically; or set `AYSAR_CHROME`). Everything else works without it.

```bash
git clone https://github.com/aysar-dev/aysar && cd aysar
cargo install --path apps/aysar-cli      # installs `aysar`
aysar doctor                             # what's installed, where projects go
```

Study something (no project created, nothing built):

```bash
aysar study https://example.com
aysar study ./some-site-folder --deep --out capture.json
aysar identify ./some-binary
```

Rebuild something:

```bash
aysar rebuild https://example.com "Recreate this website in plain HTML"
aysar rebuild - "A snake game in Rust with Bevy" --kind game
aysar rebuild ./my-old-site "Rebuild this with Astro and Tailwind" --agent codex --rounds 3
```

Then, on the project folder it prints:

```bash
aysar status   <project>      # stage, score, history
aysar check    <project>      # re-check after manual edits
aysar build    <project>      # one more refinement round
aysar evidence <project> --kind check
```

Compare any two sites without a project:

```bash
aysar compare https://original.example ./my-build
```

### Clean room or Deep

- **Clean room** (default): Aysar reads only what a visitor sees — titles, headings, links, forms, visible text, landmarks, image counts, and what the rendered page looks like (screenshots, measured colours, fonts, spacing, layout). Stylesheets, scripts, source and binaries are **not** read. Tools that would look inside refuse and tell you how to switch.
- **Deep** (`--deep`): everything is read, for your private use — styles, scripts, raw HTML, and binaries through the analysis engine.

### Coding agents

Build needs a coding agent on your machine. Aysar runs *your* agent with *your* sign-in and never handles your account.

| Agent | How Aysar runs it | Select with |
|---|---|---|
| Claude Code | `claude -p <prompt> --permission-mode acceptEdits` (override: `AYSAR_CLAUDE_ARGS`) | `--agent claude-code` |
| Codex | `codex exec --full-auto <prompt>` (override: `AYSAR_CODEX_ARGS`) | `--agent codex` |
| Anything else | your command, with `{prompt}` / `{prompt_file}` | `--agent "aider --message-file {prompt_file} --yes"` |
| None | writes `app/BUILD-PROMPT.md` and stops | `--agent dry-run` |

### For agents (MCP)

Every tool is also an MCP tool. Register Aysar once and any MCP client (Claude Code, Codex, Cursor, …) can study, rebuild and check:

```bash
aysar setup                               # prints the config snippet
claude mcp add aysar -- aysar mcp         # Claude Code, one line
```

Tools: `aysar_study`, `aysar_identify`, `aysar_rebuild`, `aysar_check`, `aysar_doctor`.

## Project layout

```
~/Documents/Aysar/<name>/
  aysar.json          request, target, mode, stage, score, history
  capture/            capture.json, screenshots/<page>-<width>.png (+ raw/ in Deep mode)
  spec/               spec.json, spec.md, prompt-round-N.md
  app/                the generated project
  checks/             check-001.json / .md, check-001-screenshots/, …
  evidence/           ledger.jsonl
  inbox/              open questions for you
```

## Repository layout

```
crates/
  aysar-core          Mode, Target, Capture, Spec, Evidence, Project
  aysar-capture       websites over HTTP and local HTML, Clean room / Deep
  aysar-analysis      Engine trait + craft-anything adapter + file identification
  aysar-spec          capture → spec (deterministic, reproducible)
  aysar-build         Agent trait, Claude Code / Codex / Script / DryRun, prompt builder
  aysar-verify        structure + content + visual comparison, actionable mismatches
  aysar-render        headless Chromium: screenshots, layout boxes, measured styles; static server
  aysar-orchestrator  Study → Spec → Build → Check → Refine
  aysar-mcp           stdio MCP server
apps/
  aysar-cli           the `aysar` binary
docs/                 ARCHITECTURE, ROADMAP, SPEC-FORMAT
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how the pieces fit and [docs/ROADMAP.md](docs/ROADMAP.md) for what's next (behaviour checks, app and game verticals, desktop app).

## Status

Alpha. What works today, verified by tests and a dry run on a sample site:

- Website study (HTTP and local), Clean room and Deep.
- Rendering in headless Chromium: full-page screenshots at 360/768/1280px, region boxes, measured colours/fonts/type scale/spacing/radii.
- Deterministic spec generation with a measured design system, per-width layout, behaviours and rules.
- Build through Claude Code, Codex, any command template, or dry run.
- Structure, content and visual checks with actionable mismatches (grouped by width); refinement rounds that feed the last check back to the builder.
- Evidence ledger; project state and history.
- CLI and MCP server.

Not yet: behaviour checks (clicking, submitting), hover/focus states, engine-driven app/game study, desktop app. These are the next phases in the roadmap.

## Responsible use

Use Aysar on software you own or are allowed to examine. It never bypasses logins, DRM, rate limits or anti-cheat. Builds recreate structure, style and behaviour from evidence; the builder is told not to copy logos, photos or text.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. The name "Aysar" and any logo in `docs/brand/` are not covered by the code license.

## Credits

Inspired by, and designed to interoperate with, [Craft Anything](https://github.com/storytold/craft-anything) by the ArtCraft team.
