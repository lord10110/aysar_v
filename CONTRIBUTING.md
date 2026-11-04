# Contributing to Aysar

Thanks for helping. Aysar is small on purpose: one trait per boundary, one file per idea, tests next to code.

## Setup

```bash
rustup update
git clone https://github.com/aysar-dev/aysar && cd aysar
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

CI runs exactly those four commands on Linux, macOS and Windows.

## Where things go

- A new way to **observe** something → `crates/aysar-capture`.
- A new external **engine** → `crates/aysar-analysis` (`impl Engine`).
- A new **coding agent** → `crates/aysar-build` (`impl Agent`), or document a command template in the README if that's enough.
- A new **check** → `crates/aysar-verify`; add a field to `Report`, compute it, adjust weights in one place.
- A new **MCP tool** → `crates/aysar-mcp` (`tool_list` + `call_tool`).
- A new **CLI command** → `apps/aysar-cli/src/main.rs`; keep logic in the crates, not in `main.rs`.

## Rules

1. **Clean room is a hard line.** Anything that reads source, scripts, stylesheets or binaries must check `Mode` and refuse in `CleanRoom`.
2. **No asset copying.** Builders are told not to copy logos, photos or text. Don't add code paths that fetch or embed the original's assets into a build.
3. **No circumvention.** Nothing that bypasses logins, DRM, rate limits or anti-cheat. PRs that do are closed.
4. **Every claim is evidence.** If your code concludes something about a target, record it with `Ledger::record` and a source.
5. **Sentences for the builder.** A check that only produces a number is half done; produce the sentence that tells the agent what to fix.
6. **Tests.** Every crate has a `tests` module; add to it. The orchestrator has an end-to-end dry-run test; extend it when you add a step.

## Commits and PRs

- Conventional-ish messages: `capture: follow <link rel=canonical>`, `verify: add visual leg`.
- One PR per idea. Describe what changes for the user, not only the code.
- Run the four commands above before opening the PR.

## Verifying an agent preset

`aysar doctor` marks presets that nobody has run live yet. To verify one: install the tool, sign in, run `aysar rebuild ./examples/bakery-site "Recreate this website" --agent <name> --rounds 1`, and open a PR that flips its `verified` flag in `crates/aysar-build/src/lib.rs` with the command output in the description.

## Releasing

Tag `vX.Y.Z` on `main`; the release workflow builds archives for five targets, attaches `SHA256SUMS.txt` and drafts notes from the commits. Bump `version` in the workspace `Cargo.toml` and add a CHANGELOG entry first.

## Reporting a bug

Open an issue with: the command you ran, the target (or a minimal stand-in), `aysar doctor` output, and the relevant `checks/*.md` or `evidence/ledger.jsonl` lines.

## Code of conduct

Be kind, be specific, assume good faith. That's it.
