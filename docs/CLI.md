# `aysar` command reference

Global flags (any command): `--deep` / `--clean-room` (mode), `--no-render` (skip Chromium).

| Command | What it does |
|---|---|
| `aysar doctor` | Lists coding agents (and the exact command each runs), the analysis engine, Chromium, the projects folder and the config file. |
| `aysar study <target> [--max-pages N] [--out capture.json]` | Observes a URL or local HTML folder/file, renders it, prints what was seen and the measured styles. No project is created. |
| `aysar identify <file>` | Says what a local file is (magic bytes + extension). |
| `aysar rebuild <target\|-> "<request>" [options]` | The whole loop: study → spec → build → check → refine. Creates a project folder. |
| `aysar spec <project> [--rule "…"]` | Regenerates the spec from the capture. |
| `aysar enrich <project> [--url …] [--model …]` | Asks a language model to add purposes, behaviours and notes (marked as suggestions). |
| `aysar build <project> [--agent …]` | One more build round, then a check. |
| `aysar check <project> [--json]` | Re-checks `app/` against the original. |
| `aysar compare <original> <build>` | Checks any two sites/folders/captures without a project. |
| `aysar status <project> [--json]` | Stage, score, agent, candidates, history, open questions. |
| `aysar evidence <project> [--kind observation\|inference\|check\|decision\|action]` | Prints the ledger. |
| `aysar ui [--port 7331] [--no-open] [--projects-dir …]` | Local dashboard. |
| `aysar mcp` | Serves the tools over MCP on stdio. |
| `aysar setup` | Prints the MCP registration snippet. |
| `aysar config [--init]` | Shows (or writes a starter) `config.toml`. |
| `aysar completions <bash\|zsh\|fish\|powershell\|elvish>` | Shell completions. |

## `rebuild` options

| Option | Meaning | Default |
|---|---|---|
| `--agent NAME` | `claude-code`, `codex`, `aider`, `gemini`, `opencode`, `goose`, `qwen`, `dry-run`, or a command template with `{prompt}` / `{prompt_file}` | config `agent`, else the first installed |
| `--agents a,b,c` | Candidate builds; the best check wins and lands in `app/` | — |
| `--rounds N` | Refinement rounds | config `rounds`, else 2 |
| `--target-score S` | Stop refining at this score | config `target_score`, else 85 |
| `--max-pages N` | Pages to study on one origin | config `max_pages`, else 5 |
| `--kind website\|app\|game` | Override detection | detected from the target |
| `--projects-dir DIR` | Where the project folder goes | config `projects_dir`, else `~/Documents/Aysar` |
| `--rule "…"` | Extra rule for the builder (repeatable) | — |

## Environment variables

| Variable | Effect |
|---|---|
| `AYSAR_CONFIG` | Path of the config file |
| `AYSAR_PROJECTS_DIR`, `AYSAR_AGENT`, `AYSAR_CHROME` | Override config values |
| `AYSAR_NO_RENDER` | Same as `--no-render` |
| `AYSAR_CLAUDE_BIN`, `AYSAR_CLAUDE_ARGS` | Claude Code binary and extra args (default `--permission-mode acceptEdits`) |
| `AYSAR_CODEX_BIN`, `AYSAR_CODEX_ARGS` | Codex binary and extra args (default `--full-auto`) |
| `AYSAR_AGENT_AIDER`, `AYSAR_AGENT_GEMINI`, … | Replace a preset's command template |
| `AYSAR_LLM_URL`, `AYSAR_LLM_MODEL`, `AYSAR_LLM_KEY` | For `aysar enrich` |
| `AYSAR_CRAFT_ANYTHING_BIN` | Path of `craft-anything-cli` for Deep-mode binary analysis |

## Exit codes

`0` success; `1` any error (the message is on stderr, prefixed `error:`).

## MCP tools

`aysar_study {target, deep?, max_pages?, render?}`, `aysar_identify {path}`, `aysar_rebuild {target, request, deep?, agent?, rounds?, projects_dir?}`, `aysar_check {project}`, `aysar_doctor {}`.
