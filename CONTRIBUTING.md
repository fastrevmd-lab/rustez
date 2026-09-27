# Contributing to rustEZ

Thanks for considering a contribution. rustEZ is an async-first Rust replacement for Juniper PyEZ, built on [rustnetconf](https://github.com/fastrevmd-lab/rustnetconf) — part of the [mechub](https://github.com/fastrevmd-lab) family of open-source, self-hosted network automation tooling. See [README.md](README.md) for what it does.

## Before you start

- Check open issues and PRs first.
- For anything larger than a small fix, open an issue to discuss the approach first.
- Hard rule across the mechub fleet: **deterministic code decides, a model may explain, a human approves.** Nothing here should let a model output drive a `commit`, `load`, or `rpc` call against a device directly. Models may draft or explain; deterministic code decides.
- Host-key verification is fail-closed by design (see README) — don't add a code path that defaults to trusting an unknown host key.

## Workspace layout

Cargo workspace with three members:

- `rustez` — the core library (`Device`, `Facts`, `Config`, RPC)
- `rustez-cli` — CLI binary
- `rustez-py` — Python bindings via PyO3

## Build and test

```sh
cargo check                     # workspace type-check
cargo test -p rustez            # unit tests, no device needed
cargo clippy -p rustez -- -D warnings
cargo clippy -p rustez-py -- -D warnings
cargo doc -p rustez --no-deps
cargo fmt --all -- --check
```

Dependency and license checks, both required in CI (`.github/workflows/security.yml`):

```sh
cargo audit
cargo deny check bans sources licenses
```

If you change a public API, also check it still compiles at the declared MSRV (see `rust-toolchain.toml` for why the CI pin and the MSRV floor are tracked separately):

```sh
cargo update -p aes --precise 0.9.2   # same pin CI uses; see the msrv job in ci.yml
cargo +1.86.0 check -p rustez
```

### Integration tests

Gated behind `#[ignore]` and require a reachable vSRX:

```sh
RUSTEZ_VSRX_HOST=<device-ip> RUSTEZ_VSRX_USER=<user> RUSTEZ_VSRX_KEY=~/.ssh/<key> \
  cargo test -p rustez -- --ignored
```

Not required for a normal contribution — skip unless you have lab access. Never point these at a production device, and never commit real hostnames, credentials, or device output; use synthetic inline test data (see the existing unit tests).

### Python bindings

If you touch `rustez-py`:

```sh
cd rustez-py
python -m venv .venv
.venv/bin/pip install --quiet --upgrade pip maturin pytest lxml
VIRTUAL_ENV="$PWD/.venv" .venv/bin/maturin develop
.venv/bin/pytest -q
```

### If you bump a version

`scripts/check_versions.py` (run in CI) requires `rustez/Cargo.toml`, `rustez-py/Cargo.toml`, and `rustez-py/pyproject.toml` to agree. Releasing, tagging, and publishing are maintainer-only steps (see `CLAUDE.md`'s Release Process) — contributors don't need to do any of that.

## Commit and PR conventions

- Keep PRs focused on one change.
- Fill out the PR template, including the exact commands you ran to verify the change.
- By opening a pull request, you're agreeing your contribution is licensed under this repository's [MIT license](LICENSE).

## Review process

Every pull request goes through a security review and a code review, then an independent test run, before anything merges. Only a maintainer merges — contributors, including anyone with write access, should not merge their own PR. CI must be green first.

## Reporting a vulnerability

Please don't open a public issue for a security vulnerability — see [SECURITY.md](SECURITY.md) for how to report one privately.

## Fixtures and test data

Never commit real device configs, hostnames, serial numbers, or credentials — synthetic or sanitized fixtures only.
