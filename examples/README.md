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
