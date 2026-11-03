# Roadmap

Phases are ordered so that each one ships something usable on its own. Checkboxes are the plan; the README's Status section is the truth.

## Phase 0 — Foundation ✅ (this release)

- [x] Cargo workspace, 8 crates + CLI, CI, dual license
- [x] `Mode` (Clean room / Deep) enforced in capture and analysis
- [x] `Capture` for websites (HTTP, same-origin BFS crawl) and local HTML
- [x] `Spec` format v1 + deterministic generator + Markdown rendering
- [x] `Agent` gateway: Claude Code, Codex, command template, dry-run
- [x] `Verify`: structure + content legs, actionable mismatches, per-page table
- [x] Orchestrator with refinement rounds that feed the last check into the prompt
- [x] Evidence ledger (JSONL) with support graph
- [x] MCP server (stdio) with 5 tools; `aysar setup`
- [x] Tests for every crate; end-to-end dry-run test

## Phase 1 — Websites, end to end (4–6 weeks)

Goal: beat the state of the art on the website vertical.

- [ ] **Renderer-based capture** (Deep *and* Clean room): headless Chromium via `chromiumoxide`, or Playwright through an MCP sidecar. Screenshots at 360/768/1280, computed styles (colours, fonts, spacing), bounding boxes of landmarks and headings.
- [ ] **Design-system extraction** from measured styles: palette, type scale, spacing scale, radius; written into `Spec.design` with evidence.
- [ ] **Visual check leg**: perceptual hash per viewport + layout-box IoU; mismatches like "hero section is 40% taller at 360px".
- [ ] **State capture**: hover/focus/open-menu states recorded as extra screenshots.
- [ ] **Behaviour check leg v1**: for each `Behavior` in the spec (forms, navigation), drive both sites and compare: does the link reach the page, does the form validate, does submit give feedback.
- [ ] **Score weights** updated to include visual and behaviour.
- [ ] Capture of `robots.txt` and a polite crawl rate; `--max-pages` default stays small.

## Phase 2 — Agent-agnostic for real (3 weeks)

- [ ] ACP (Agent Client Protocol) adapter so Gemini CLI, opencode, goose and others plug in without a template.
- [ ] Verified adapters with a recorded smoke test each: Claude Code, Codex, Aider, Gemini CLI, opencode.
- [ ] Local models for analysis and spec enrichment (Ollama / llama.cpp / any OpenAI-compatible endpoint): `aysar enrich <project>`.
- [ ] Parallel builds: two agents on the same spec, two checks, pick the higher score (`--agents claude-code,codex`).
- [ ] Approval cards: commands outside an allow-list stop and write to `inbox/`; `--yes` and `--answer` on the CLI.

## Phase 3 — Apps (4–6 weeks)

- [ ] Engine-driven study in Deep mode through `craft-anything-cli` (functions, strings, resources, UI layouts) → `Spec` screens and flows.
- [ ] Window capture in Clean room (a picture of the running app) → screens and visible controls.
- [ ] Interaction recording: a short session on the original becomes `Behavior`s.
- [ ] Behaviour checks for apps: run the build, replay the recorded session, compare outcomes.
- [ ] Stacks: Tauri, Flutter, SwiftUI/Kotlin, web.

## Phase 4 — Games (4–6 weeks)

- [ ] Rules and physics extraction into `Spec` (entities, inputs, scoring, win/lose).
- [ ] Builders for Bevy, Godot, Unity; a "self-test" scene the agent must make pass.
- [ ] Gameplay checks: scripted inputs on both, compare state traces.

## Phase 5 — Workbench and desktop app (ongoing)

- [ ] Evidence graph browser (what supports what; what would change if X were wrong).
- [ ] Desktop app (egui or Tauri) with Simple and Advanced modes over the same crates.
- [ ] Project list, inbox, approval cards, live check scores.

## Phase 6 — Release and community

- [ ] Installers: Windows (msi), macOS (dmg, signed), Linux (AppImage, deb, rpm), FreeBSD.
- [ ] `cargo install aysar` from crates.io.
- [ ] Contribute capture and verification improvements upstream to craft-anything where they fit.
- [ ] Docs site; sample projects; a "beat the score" leaderboard for agents.

## Non-goals

- Bypassing logins, DRM, rate limits or anti-cheat. Ever.
- Copying assets or text from the original into a build.
- Writing our own disassembler or decompiler. Engines exist; we orchestrate them.
