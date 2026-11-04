# Changelog

All notable changes to Aysar. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

## [0.2.0] — 2026-11-04

### Added
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
