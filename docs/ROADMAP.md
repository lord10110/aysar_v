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
- [x] Tests for every crate; end-to-end dry-run test; live Chromium render test (skips when no browser)

## Phase 1 — Websites, end to end ✅

Goal: beat the state of the art on the website vertical.

- [x] **Renderer-based capture** (Deep *and* Clean room): headless Chromium via `headless_chrome`, found on the machine, never downloaded. Full-page screenshots at 360/768/1280, computed styles (colours, fonts, type scale, spacing, radii), bounding boxes of landmarks, headings, forms and images. Local folders served on loopback so `/path` links work.
- [x] **Design-system extraction** from measured styles into `Spec.design` with evidence; per-page, per-width layout regions in `Spec.pages[].viewports`.
- [x] **Visual check leg**: dHash per viewport + region layout + page height + palette and font comparison; mismatches like `at 360/768/1280px: <footer> is 66px tall in the original and 18px in the build`, grouped by width.
- [x] **Score weights** include visual (0.4 / 0.1 / 0.5).
- [x] Headings compared by level structure first, since the builder must not copy text.
- [x] **State capture**: hover and focus measured on nav links, buttons and inputs through real CDP mouse moves and `focus()` (open-menu states: later).
- [x] **Behaviour check leg v1**: links fetched from inside the page, forms validated empty and filled, required counts, submit buttons, hover/focus parity; weights 0.35/0.10/0.35/0.20.
- [x] Side-by-side pictures in `checks/` (original | build | difference).
- [x] `robots.txt` (Allow/Disallow/Crawl-delay) and a 300 ms polite delay; `--max-pages` default stays small.
- [ ] Tune weights against a small corpus of real sites and agent builds.

## Phase 2 — Agent-agnostic for real ✅ (mostly)

- [x] Presets for Aider, Gemini CLI, opencode, goose, Qwen as command templates; `AYSAR_AGENT_<NAME>` overrides; `doctor` shows the command and whether it is verified.
- [x] Claude Code verified live (full rebuild, 92/100). Codex 0.166, Gemini 0.66, opencode 1.19, Qwen 0.26 installed and invoked: flags parse, stop at sign-in. Aider/goose from docs.
- [ ] End-to-end runs of the other agents with real accounts (contributions welcome — see CONTRIBUTING).
- [ ] ACP (Agent Client Protocol) adapter so agents plug in without a template.
- [x] `aysar enrich`: any OpenAI-compatible endpoint adds purposes, behaviours and notes as 0.6-confidence inferences.
- [x] Candidate builds: `--agents a,b` builds each, checks each, keeps the best (sequential today; parallel later).
- [x] Config file with `AYSAR_*` overrides.
- [ ] Approval cards: commands outside an allow-list stop and write to `inbox/`; `--yes` and `--answer` on the CLI.

## Phase 3 — Apps (in progress)

- [x] Self-test contract: `aysar-build.json {run, selftest}`; `check` runs it headlessly with a timeout and folds `PASS`/`FAIL` lines into the behaviour leg.
- [x] App-specific builder deliverables (screens, flows, validation, headless self-test).
- [x] Engine-driven study in Deep mode through `craft-anything-cli` 0.4.0: triage, overview, functions, strings; Android/Java/.NET/Apple routes for components, classes and resources → the spec's Program section. Verified live on a real ELF.
- [ ] Window capture in Clean room (a picture of the running app) → screens and visible controls.
- [ ] Interaction recording: a short session on the original becomes `Behavior`s; replay on the build.

## Phase 4 — Games (in progress)

- [x] Game-specific builder deliverables: controls, rules, scoring, win/lose, restart, each with a `PASS`/`FAIL` line; `GAMEPLAY.md`.
- [ ] Rules and physics extraction into `Spec` from a studied original (entities, inputs, scoring).
- [ ] Gameplay checks: scripted inputs on both, compare state traces.

## Phase 5 — Workbench ✅ (web) / desktop (later)

- [x] Local dashboard `aysar ui`: projects, stages, per-leg scores, mismatches, side-by-side pictures, spec, checks, screenshots, evidence, history, new-project form, build/check buttons with live logs.
- [ ] Evidence graph browser (what supports what; what would change if X were wrong).
- [ ] Native desktop shell (Tauri) around the same dashboard.

## Phase 6 — Release and community (in progress)

- [x] Release workflow: tags build Linux (x86_64, aarch64), macOS (arm64, x86_64) and Windows archives with checksums.
- [x] CHANGELOG, SECURITY, CODE_OF_CONDUCT, issue and PR templates, Dependabot.
- [x] Black-box CLI tests; CI on three platforms with Chrome.
- [ ] Installers: msi, dmg (signed), AppImage/deb/rpm.
- [ ] `cargo install aysar` from crates.io.
- [ ] Contribute capture and verification improvements upstream to craft-anything where they fit.
- [ ] Docs site; sample projects; a "beat the score" leaderboard for agents.

## Non-goals

- Bypassing logins, DRM, rate limits or anti-cheat. Ever.
- Copying assets or text from the original into a build.
- Writing our own disassembler or decompiler. Engines exist; we orchestrate them.
