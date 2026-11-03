# Guide for coding agents working on this repository

You are editing **Aysar**, a Rust workspace. Read `docs/ARCHITECTURE.md` first.

## Build & test

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all
```

All four must pass before you finish.

## Map

- `crates/aysar-core` — shared types. Don't add I/O here beyond the ledger and project files.
- `crates/aysar-capture` — HTML observation. `html::parse_page` is the single parser; Clean room vs Deep is decided there.
- `crates/aysar-analysis` — `Engine` trait and adapters. `Engine::run` must refuse in Clean room.
- `crates/aysar-spec` — deterministic `Capture → Spec`. Keep it deterministic; AI enrichment is a separate step.
- `crates/aysar-build` — `Agent` trait, adapters, `prompt::build_prompt`.
- `crates/aysar-verify` — `compare(original, build) -> Report`. Mismatches are sentences.
- `crates/aysar-orchestrator` — `study / specify / build / check / rebuild`.
- `crates/aysar-mcp` — stdio MCP server. `handle()` is pure; keep it that way.
- `apps/aysar-cli` — clap only; no logic.

## Invariants (do not break)

1. Clean room never reads source, scripts, stylesheets or binaries.
2. The builder never sees the original's files or raw capture — only the prompt.
3. The spec is reproducible from the capture.
4. Every conclusion about a target is recorded as evidence with a source.
5. No code that copies the original's assets into `app/`, and nothing that bypasses logins, DRM, rate limits or anti-cheat.

## Style

- Plain words in user-facing strings; no jargon in CLI output.
- Small functions, one idea per file, tests in a `tests` module next to the code.
- Prefer adding a field to an existing struct over a new parallel type.
