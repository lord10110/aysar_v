# Examples

## bakery-site

A two-page sample site (home + contact form). Try the whole loop without any AI installed:

```bash
aysar rebuild ./examples/bakery-site "Recreate this website in plain HTML" --agent dry-run --projects-dir /tmp/aysar-demo --kind website
cat /tmp/aysar-demo/bakery-site/spec/spec.md
cat /tmp/aysar-demo/bakery-site/app/BUILD-PROMPT.md     # hand this to any coding agent
```

Then, with Claude Code or Codex installed:

```bash
aysar build /tmp/aysar-demo/bakery-site --agent claude-code
cat /tmp/aysar-demo/bakery-site/checks/check-001.md
```

## selftest-contract

The `aysar-build.json` contract that builders of apps and games follow. Try it:

```bash
mkdir -p /tmp/aysar-demo && aysar rebuild - "A snake game" --agent dry-run --kind game --projects-dir /tmp/aysar-demo
cp examples/selftest-contract/* /tmp/aysar-demo/a-snake-game/app/
aysar check /tmp/aysar-demo/a-snake-game        # behaviour 100/100 from four PASS lines
```
