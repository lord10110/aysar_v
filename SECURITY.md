# Security

## Reporting

Please report vulnerabilities privately through GitHub's "Report a vulnerability" button on the repository's Security tab, or by email to the maintainers listed in `Cargo.toml`. Give us a few days to respond before disclosing publicly.

## What Aysar does and does not do

- **Network**: Aysar fetches the pages you point it at (respecting `robots.txt`), talks to the coding agent you installed, and — only when you run `aysar enrich` — to the model endpoint you configured. Nothing else. No telemetry.
- **Dashboard**: `aysar ui` listens on `127.0.0.1` only, with no authentication. It can start builds and run self-tests from projects in your projects folder, so do not expose that port.
- **Self-tests and agents run code**: `check` executes the `selftest` command a build declares in `aysar-build.json`, and `build` runs your coding agent with write access to `app/`. Treat a project folder like any other code you run.
- **Clean room** never reads a target's source, scripts, stylesheets or binaries. Deep mode does, on your machine, for your own use.
- Aysar never bypasses logins, DRM, rate limits or anti-cheat, and refuses to add code paths that do.

## Supported versions

The latest minor release receives fixes.
