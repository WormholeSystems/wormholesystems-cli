# wormholesystems-cli (`wsctl`)

Interactive setup wizard for the [Wormhole Systems container stack](https://github.com/WormholeSystems/wormholesystems-containers). It automates the README setup steps:

- clones the containers repo (with submodules) if you're not already in it
- asks for domains, contact info, EVE developer credentials and DB settings
- generates all secrets (DB passwords, Reverb ID/key/secret, Laravel `APP_KEY`)
- writes `.env` and `dockerfiles/mysql/.env` with guaranteed-matching DB credentials, patched from the upstream example templates so comments and untouched keys stay intact
- creates the `web` Traefik network, builds and starts the stack
- runs the init steps (`sde:download`, `migrate --seed`, `optimize`)

## Usage

```bash
wsctl                     # version info and command list
wsctl setup               # wizard (alias: init); uses cwd if it's the containers repo, else offers to clone
wsctl setup --dir /path/to/wormholesystems-containers
wsctl update              # refresh EVE static data of a running instance (cwd or --dir)
wsctl doctor              # check git / docker / docker compose are available
wsctl about               # version and project links
```

`wsctl update` verifies it's in a configured checkout, detects the stack from `APP_ENV`, checks the `app`/`mysql` containers are running, then runs the upstream update sequence (`sde:download`, `migrate --force`, `sde:seed`).

Production mode drives the full Traefik+SSL flow. Local test mode runs against `docker-compose.test.yml` directly (no include editing) and, when ports 80/8080 are taken, remaps them via a generated `docker-compose.wsctl-ports.yml` override — the chosen ports flow into `APP_URL`, the EVE callback guidance, and the websocket client config.

An interrupted setup (failed build, Ctrl-C) is resumable: progress is persisted per step in `.wsctl-state.json` (no secrets) inside the containers repo, and the next `wsctl setup` offers to continue where it stopped.

## Architecture

Three layers, split for testability:

- `plan` — pure: answers → files to write + command steps to run. All decisions live here; unit tests assert exact command lines.
- `wizard` — interactive: prompts, preflight gates (docker daemon, busy ports, buildable checkout), orchestration.
- `exec` + `state` — execution through a mockable `Runner` trait, with per-step progress persistence for resume.

`envfile`/`secrets` are pure helpers; `docker` wraps process spawning and daemon checks.

## Testing against upstream

The containers repo is vendored as a git submodule at `upstream/wormholesystems-containers`:

```bash
git submodule update --init --recursive
cargo test
```

`tests/upstream_templates.rs` patches the *real* upstream example templates with the wizard's full key set and fails if upstream renamed/removed a key we manage, added a new `<fill in ...>` placeholder we don't cover, or dropped a compose service the wizard relies on. It also checks the pinned app source (nested submodule): the ESI scope list the wizard tells users to enable must match the app's seeded defaults and frontend, and `URL::forceHttps()` must stay environment-guarded. CI runs all of this weekly against upstream HEAD (`git submodule update --remote`) to catch drift while this repo is idle.

For a full interactive end-to-end run without docker:

```bash
./scripts/e2e.exp /path/to/scratch-copy-of-templates
```

## Distribution

Binaries are not committed to git. Tagging `v*` builds `wsctl` for Linux/macOS (x86_64 + aarch64) and attaches the binaries to a GitHub Release (`.github/workflows/release.yml`).

One-line install (downloads the release binary for the platform, then offers to run the wizard):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://install.wormhole.systems | sh
```

Pin a version with `WSCTL_VERSION=v0.1.0 curl ... | sh`. The script installs to `/usr/local/bin` (or `~/.local/bin`) and reattaches the wizard to `/dev/tty` since stdin is the curl pipe. The raw GitHub URL works too: `https://raw.githubusercontent.com/WormholeSystems/wormholesystems-cli/main/install.sh`.

`install.wormhole.systems` is GitHub Pages serving `install.sh` as the root document (`.github/workflows/pages.yml`). One-time setup after pushing the repo:

1. Repo Settings → Pages → Source: **GitHub Actions**, Custom domain: `install.wormhole.systems` (enforce HTTPS once the cert is issued).
2. DNS: `CNAME install.wormhole.systems → wormholesystems.github.io`.

The containers repo gets `dist/setup.sh` committed as `./setup.sh` plus a `.wsctl-version` file pinning the wizard release that revision was tested against. Users then run:

```bash
git clone --recurse-submodules https://github.com/WormholeSystems/wormholesystems-containers.git
cd wormholesystems-containers
./setup.sh
```

Keeping things in sync is a one-line bump of `.wsctl-version` in the containers repo whenever a new wizard release is cut.
