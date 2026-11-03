# Architecture

Aysar is a pipeline with a spec in the middle and evidence underneath.

```
┌─────────────────────────────────────────────────────────────────┐
│  Interfaces      aysar (CLI)   ·   aysar mcp (MCP server)        │
│                  desktop app (planned, phase 5)                  │
├─────────────────────────────────────────────────────────────────┤
│  Orchestrator    study → specify → build → check → (refine)     │
│                  each step is a function; the loop is `rebuild` │
├──────────────┬──────────────┬──────────────┬────────────────────┤
│  Capture     │  Analysis    │  Spec        │  Verify            │
│  what a      │  Engine      │  capture →   │  original capture  │
│  visitor     │  trait +     │  Spec (IR)   │  vs build capture  │
│  sees; Deep  │  craft-      │  determin-   │  structure/content │
│  reads more  │  anything    │  istic       │  (+visual, +behav) │
├──────────────┴──────────────┴──────────────┴────────────────────┤
│  Build / Agent gateway    Agent trait: ClaudeCode · Codex ·      │
│                           Script(template) · DryRun              │
├─────────────────────────────────────────────────────────────────┤
│  Core   Mode · Target · Kind · Capture · Spec · Evidence ledger  │
│         Project (on-disk layout + state + history)               │
└─────────────────────────────────────────────────────────────────┘
```

## Principles

1. **The spec is the contract.** Observation (`Capture`) and construction (the agent's prompt) never touch directly. Everything flows through `Spec`, which is a plain JSON file with a Markdown rendering. Humans can edit it; agents can read it; tests can diff it.

2. **Deterministic by default, enriched optionally.** `aysar-spec::generate` is rule-based: the same capture always yields the same spec. An AI enrichment pass (purposes, behaviours it can infer) is planned as a separate, clearly-labelled step that *adds* to the deterministic base and records its additions as `Inference` evidence with a lower confidence.

3. **Mode is enforced at the boundary, not by convention.** `Mode::CleanRoom` is checked inside `aysar-capture` (what gets parsed) and inside `aysar-analysis::Engine::run` (which refuses). A tool cannot accidentally read source in Clean room.

4. **Agents are interchangeable.** The `Agent` trait is three methods: `name`, `available`, `run(workdir, prompt)`. The orchestrator never knows which agent it is driving. The agent runs *inside `app/`* and never sees the original's files or raw capture; it only sees the prompt (spec + rules + last check).

5. **Checks are sentences.** `aysar-verify` produces numbers for the score and *sentences* for the builder. The next round's prompt includes those sentences verbatim. This is the refinement loop.

6. **Everything leaves a trace.** Observations, inferences, checks, actions and decisions go to `evidence/ledger.jsonl` through `Ledger::record`. `Ledger::supporters_of` walks the support graph. The ledger is append-only JSON Lines today; it can move to SQLite or a graph store without changing callers.

7. **Engines, not rewrites.** Aysar does not contain a disassembler. `aysar-analysis::Engine` wraps external engines. The first adapter targets `craft-anything-cli`; the `NullEngine` keeps the pipeline running when nothing is installed.

## Crates

| Crate | Depends on | Responsibility |
|---|---|---|
| `aysar-core` | — | Types shared by everyone. No I/O except the ledger and project files. |
| `aysar-capture` | core, ureq, scraper, url | HTTP crawl (same origin, BFS, `max_pages`) and local HTML; `html::parse_page` is the single parser. |
| `aysar-analysis` | core | `Engine` trait, `CraftAnything` adapter, `NullEngine`, `identify()` by magic bytes. |
| `aysar-spec` | core | `generate(SpecInput, Option<&Ledger>) -> Spec`; `detect_stack(request)`. |
| `aysar-build` | core | `Agent` trait and adapters; `prompt::build_prompt(spec, round, previous_check, existing_files)`. |
| `aysar-verify` | core | `compare(original, build) -> Report`; per-page scores and mismatches. |
| `aysar-orchestrator` | all above | `new_project`, `study`, `specify`, `build`, `check`, `rebuild`; `Reporter` for progress. |
| `aysar-mcp` | core, capture, analysis, orchestrator, build | stdio JSON-RPC 2.0 server; `handle(req)` is pure and tested. |
| `aysar-cli` | everything | `clap` front end. |

## Data flow for `aysar rebuild <target> "<request>"`

```
Target::parse(target) ──► new_project() ──► <root>/aysar.json
        │
        ▼
study()      capture(target, mode) ──► capture/capture.json
             + Observation evidence per page
        │
        ▼
specify()    generate(capture) ──► spec/spec.json, spec/spec.md
             + Inference evidence per behaviour
        │
        ▼ (round = 1..rounds)
build()      build_prompt(spec, round, last_check, files)
             ──► spec/prompt-round-N.md
             agent.run(app/, prompt) ──► app/**
             + Action evidence
        │
        ▼
check()      capture_local(app/, Deep) ──► compare(original, build)
             ──► checks/check-NNN.{json,md}
             + Check evidence; score stored in aysar.json
        │
        ▼
stop when score ≥ target_score or rounds exhausted ──► Stage::Ready
```

## Scoring

`Report.score = 0.7 × structure + 0.3 × content` today, where

- `structure` = mean over original pages of (title, headings, landmarks, navigation, forms) similarity,
- `content` = mean over pages of word-count and image-count ratio.

Two legs are reserved and will change the weights when they land:

- **visual**: screenshots at 360 / 768 / 1280 px, perceptual hash + layout-box IoU,
- **behaviour**: drive both sites (click, fill, submit, navigate), extract a state machine each, compare.

Similarities: Jaccard over token or item sets; LCS/max-length for ordered headings; min/max ratio for counts.

## Extending

- **New agent:** implement `Agent` in `aysar-build`, or just pass a command template with `--agent "…{prompt_file}…"`.
- **New engine:** implement `Engine` in `aysar-analysis`; add it to `detect()`.
- **New capture source (e.g. a window recorder):** add a module in `aysar-capture` that produces `PageCapture`s (or a new capture type in core if pages don't fit).
- **New check leg:** add fields to `Report`, compute in `compare`, and adjust the weights in one place.
- **New MCP tool:** add to `tool_list()` and `call_tool()` in `aysar-mcp`.
