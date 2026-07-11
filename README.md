# wsctl

[![CI](https://github.com/WormholeSystems/wormholesystems-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/WormholeSystems/wormholesystems-cli/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/WormholeSystems/wormholesystems-cli)](https://github.com/WormholeSystems/wormholesystems-cli/releases/latest)
[![License](https://img.shields.io/github/license/WormholeSystems/wormholesystems-cli)](LICENSE)
[![Stack](https://img.shields.io/badge/stack-wormholesystems--containers-blue)](https://github.com/WormholeSystems/wormholesystems-containers)
[![App](https://img.shields.io/badge/app-WormholeSystems-blue)](https://github.com/WormholeSystems/WormholeSystems)

One tool to set up and manage a self-hosted [Wormhole Systems](https://wormhole.systems) instance. It automates the entire [container stack](https://github.com/WormholeSystems/wormholesystems-containers) setup — cloning, configuration, secrets, docker build, database initialization — through an interactive wizard, and keeps the instance's EVE data updated afterwards.

## Install

```bash
curl --proto '=https' --tlsv1.2 -sSf https://install.wormhole.systems | sh
```

Detects your platform, installs the latest `wsctl` release binary and offers to start the setup right away. Pin a version with `WSCTL_VERSION=v0.3.0 curl ... | sh`, or grab a binary directly from the [releases](https://github.com/WormholeSystems/wormholesystems-cli/releases).

**Requirements:** git, Docker with Compose v2.24+, and for production a domain pointing at your server with ports 80/443 open. You will also need an [EVE developer application](https://developers.eveonline.com/) — the wizard tells you exactly how to configure it (callback URL and scopes) when you get there.

## Usage

### Set up a new instance

```bash
wsctl setup
```

The wizard walks through everything the stack needs:

- clones `wormholesystems-containers` (with submodules) into a directory you choose, or uses the checkout you run it from
- production (Traefik, automatic SSL) or local test mode (plain HTTP on localhost)
- asks for domains, contact info and EVE application credentials; generates all secrets (database passwords, Reverb keys, Laravel `APP_KEY`)
- writes `.env` and `dockerfiles/mysql/.env` with guaranteed-matching credentials
- builds and starts the stack, then initializes the database (EVE SDE download, migrations)

Preflight gates catch the usual traps before they cost you a 10-minute build: Docker not running, ports already in use (in local mode you can remap them — the EVE callback URL guidance follows along), or a checkout missing its submodules.

Interrupted? A failed build or Ctrl-C is not a restart: progress is saved per step in `.wsctl-state.json`, and the next `wsctl setup` offers to resume exactly where it stopped.

### Keep it updated

```bash
wsctl update
```

Run inside the checkout (or pass `--dir`). Verifies the stack is running, detects production vs local from `APP_ENV`, then refreshes the EVE static data: `sde:download`, `migrate --force`, `sde:seed`.

### Other commands

```bash
wsctl          # version info and command list
wsctl doctor   # check git / docker / docker compose / daemon
wsctl about    # version and project links
```

## Related repositories

| Repository | What it is |
|---|---|
| [wormholesystems-containers](https://github.com/WormholeSystems/wormholesystems-containers) | The docker stack this tool sets up (Traefik, frankenPHP, MySQL, Redis, Reverb) |
| [WormholeSystems](https://github.com/WormholeSystems/WormholeSystems) | The Laravel application itself |

## Development

```bash
git submodule update --init --recursive
cargo test
cargo run -- setup   # or any other command
```

### Architecture

Three layers, split for testability:

- `plan` — pure: answers → files to write + command steps to run. All decisions live here; unit tests assert exact command lines.
- `wizard` — interactive: prompts, preflight gates, orchestration.
- `exec` + `state` — execution through a mockable `Runner` trait, with per-step progress persistence for resume.

`envfile`/`secrets` are pure helpers; `docker` wraps process spawning and daemon checks.

### Testing against upstream

The containers repo is vendored as a git submodule at `upstream/wormholesystems-containers`. `tests/upstream_templates.rs` runs the wizard's env generation against the *real* upstream templates and fails when upstream renames/removes a key we manage, adds a placeholder we don't cover, drops a compose service, changes the app's default ESI scopes, or unguards `URL::forceHttps()`. CI re-runs this weekly against upstream HEAD, so drift gets caught even while this repo is idle.

For a scripted interactive run without docker: `./scripts/e2e.exp <scratch-copy-of-templates>`.

### CI and releases

GitHub Actions (`.github/workflows/`):

- **ci.yml** — `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` on every push/PR, plus the weekly upstream-HEAD drift run
- **release.yml** — tagging `v*` builds `wsctl` for Linux/macOS (x86_64 + aarch64) and attaches the binaries to a GitHub Release
- **pages.yml** — publishes `install.sh` to GitHub Pages as `install.wormhole.systems` (root document *is* the script, so `curl | sh` works on the bare domain)

Binaries are never committed to git. The containers repo can additionally ship `dist/setup.sh` as `./setup.sh` with a `.wsctl-version` pin, so `git clone && ./setup.sh` works without curl.

## License

[MIT](LICENSE)
