# Changelog

All notable changes to Aysar. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

## [0.2.2] — 2026-11-04

### Added
- **Built-in `api` builder**: no coding-agent program needed. Any OpenAI-compatible endpoint — Ollama, LM Studio, llama.cpp locally, or OpenAI / OpenRouter / Groq hosted — returns the project as `<<<FILE path>>> … <<<END>>>` blocks that Aysar writes into `app/`; refinement rounds resend the current files with the check's mismatches. Paths are validated; stray code fences are stripped. `--agent api`, or `agent = "api"` plus `[llm]` in the config. Picked automatically when no coding agent is installed and the model server answers.
- `aysar doctor` explains the three ways to get a builder when none is found.

## [0.2.1] — 2026-11-04

### Fixed
- Linux CI and locked-down kernels: Chromium's sandboxed launch is verified by actually rendering a page, with a no-sandbox fallback; `AYSAR_CHROME_NO_SANDBOX=1` skips the first attempt. Ubuntu CI went from 29 minutes to under 8.
- Release builds the macOS Intel binary on the Apple-silicon runner (cross-compiled) instead of waiting for scarce `macos-13` runners.

### Added
- `AYSAR_RENDER_TRACE=1` prints the duration of each browser step.

## [0.2.0] — 2026-11-04

### Verified live
- The whole loop with **Claude Code** as the builder: 92/100 on the sample site in one round, zero mismatches.
- **Program study** through craft-anything-cli 0.4.0 on a real ELF (`/usr/bin/zip`): format, dependencies, imports, 223 functions, 1,166 strings, user-facing messages.
- Codex 0.166, Gemini CLI 0.66, opencode 1.19 and Qwen Code 0.26 installed and invoked with the preset commands: they parse and stop at sign-in. Codex's `--full-auto` no longer exists in `exec`; the preset is now `--sandbox workspace-write --skip-git-repo-check`. Qwen's `-p` is deprecated; the prompt is positional.

### Added
- **Deep-mode program study** (`aysar-analysis::study`): routed by format, distilled into a `ProgramStudy` (what, platform, dependencies, imports, counts, UI text, feature names, components, resources, tools run) and rendered as the spec's **Program** section; behaviours derived from components.
- `Verified` levels per agent in `aysar doctor` and the dashboard.
- **Rendering** in headless Chromium found on the machine (never downloaded): full-page screenshots at 360/768/1280 px, region boxes, measured colours, fonts, type scale, spacing and radii; `StaticServer` so local folders render with real `/path` links.
- **Behaviour probes**: same-origin links fetched from inside the page, forms validated empty and filled (never submitted), real hover and focus on nav links, buttons and inputs.
- **Visual and behaviour check legs**, blended with structure and content (`0.35 / 0.10 / 0.35 / 0.20`); mismatches grouped by width; headings compared by level structure first.
- **Side-by-side pictures** (original | build | difference) for every page and width in `checks/`.
- **Measured design system** and per-width layout regions in the spec and the builder prompt.
- **Agent presets**: aider, gemini, opencode, goose, qwen (`AYSAR_AGENT_<NAME>` overrides); `aysar doctor` shows each command and whether it has been verified live.
- **Candidate builds**: `--agents a,b` builds one candidate per agent, checks each, keeps the best in `app/`.
- **`aysar enrich`**: any OpenAI-compatible endpoint (Ollama, llama.cpp, LM Studio, OpenRouter, OpenAI) adds page purposes, behaviours and design notes as 0.6-confidence inferences.
- **Self-tests for apps and games**: builders ship `aysar-build.json` with `run` and `selftest`; `check` runs the self-test and folds `PASS`/`FAIL` lines into the behaviour leg.
- **Config file** `~/.config/aysar/config.toml` with `AYSAR_*` overrides; `aysar config [--init]`.
- **`aysar ui`**: a local dashboard — projects, stages, per-leg scores, mismatches, side-by-side pictures, spec, checks, evidence, history, and buttons to build, check or start a project; light and dark.
- `aysar check --json`, `aysar status --json`, `aysar completions <shell>`, `--no-render`.
- robots.txt (Allow/Disallow/Crawl-delay) and a polite delay between requests.
- Release workflow for Linux (x86_64, aarch64), macOS (arm64, x86_64) and Windows; CLI integration tests.

### Security (from an independent review before release)
- Dashboard: loopback `Host` required (DNS rebinding), POSTs must be JSON with an `X-Aysar` header (cross-site requests), strict project names (Windows paths included), only known agent names from the page, one job per project; `--bind` for containers with a warning.
- Self-test runner drains pipes on threads and kills the whole process group on timeout (no deadlock, no orphans).
- Chromium runs sandboxed when it can; the `--no-sandbox` fallback is used only if the sandboxed launch fails (root in containers).
- A single-file target is served from a scratch folder, not its parent directory.

### Fixed
- Panic on non-ASCII CSS in Deep mode and in every check of a build.
- Agent command templates kept their case only in part; now they are passed as written (names are still matched case-insensitively).
- Windows: `.cmd`/`.bat` shims run through `cmd /C`; very large prompts go to Claude Code on stdin.
- The behaviour probe ran at every width (up to 120 requests per page); it now runs once, paced, capped at 25 links.
- `detect_stack` matched inside words ("trust" → Rust); whole words only now, and bare "go" is no longer a language.
- `dist/` folders are studied like any other; MCP `aysar_study` honours `render: false`.

### Changed
- Weights: with a renderer the visual leg carries 0.50 (0.35 when behaviour is also present).
- Styles are sampled at the widest viewport; mobile emulation is never used (it would lay out at 980 px).

## [0.1.0] — 2026-11-03

### Added
- Workspace with `aysar-core`, `aysar-capture`, `aysar-analysis`, `aysar-spec`, `aysar-build`, `aysar-verify`, `aysar-orchestrator`, `aysar-mcp` and the `aysar` CLI.
- Clean room / Deep modes enforced in capture and analysis.
- Deterministic spec generation with behaviours and rules; Markdown rendering for prompts.
- Agent gateway: Claude Code, Codex, command templates, dry run.
- Structure and content checks with actionable sentences; refinement rounds feed the last check back to the builder.
- Evidence ledger (JSONL) with a support graph; project state and history.
- MCP server (stdio) with `aysar_study`, `aysar_identify`, `aysar_rebuild`, `aysar_check`, `aysar_doctor`.
