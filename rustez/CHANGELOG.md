# Changelog

All notable changes to the `rustez` crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.10.0] — 2026-05-06

### Fixed

- **PyDevice config methods bypass timeout protection** (PR #19) — `config_diff`, `config_commit`, and `config_rollback` in the Python bindings now route through `ConfigManager` instead of calling `client_mut()` directly, restoring per-RPC timeout wrapping.
- **`parse_cli_output` dead code** — removed unreachable duplicate `find("<output>")` block.
- **`validate_xml_name` accepts invalid names** — now rejects names starting with digits or hyphens per the XML specification.
- **Python `Config.commit(comment=...)` silently ignored** — wired through to native `commit_with_comment`.
- **Python `Config.diff(rb_id=N)` silently ignored** — `rb_id` parameter now passed to native layer.
- **`classify_error` misclassification** — reordered pattern matching; config-load errors now detected before generic timeout/connect checks; removed overly broad `"config"`/`"load"` catch-all.
- **Facts parsers silently swallow XML errors** — all four parsers now emit `tracing::warn!` on parse failures.

### Added

- **`Device::is_config_db_open()`** — public accessor for config database open state.
- **`cargo audit` in CI** — dependency vulnerability scanning runs on every PR, with `RUSTSEC-2023-0071` (rsa timing side-channel) ignored until upstream fix.
- **`cargo clippy` for `rustez-py`** in CI — previously only linted the core crate.
- **Least-privilege CI permissions** — `permissions: contents: read` added to CI workflow.

### Changed

- Bumped `rustnetconf` dependency to `0.10` — gains credential zeroization, XML fragment validation, built-in RPC timeout support, ProxyCommand shell injection fix, and max read buffer limit.
- Synced `rustez-py` version to match core crate.
- Removed unused `to_netconf_err` function from Python bindings.

[0.10.0]: https://github.com/fastrevmd-lab/rustEZ/compare/v0.9.0...v0.10.0

## [0.9.0] — 2026-05-04

### Added

- **SSH connectivity options** (PR #18) surfaced from rustnetconf 0.9:
  - `DeviceBuilder::jump_hosts(Vec<JumpHostConfig>)` — multi-hop bastion chain (OpenSSH `ProxyJump`).
  - `DeviceBuilder::proxy_command(&str)` — OpenSSH-style `ProxyCommand` with `%h` / `%p` substitution.
  - `Device::connect_via_ssh_config(alias)` and `Device::connect_via_ssh_config_at(path, alias)` — resolve a `Host` alias from `~/.ssh/config` (or an explicit path) into a populated `DeviceBuilder`. Subsequent builder calls override resolved values.
  - Re-exports `JumpHostConfig`, `SshConfigError`, `SshConfigFile`, and `ResolvedHost` from the crate root.
  - New `RustEzError::SshConfig` variant (mapped via `#[from]`) so callers don't need to import rustnetconf.
- **`ConfigManager::commit_with_comment`** (PR #16) — commit with an attached commit log comment.
- **`Serialize` derive** on `Facts`, `Personality`, and `RouteEngine` (PR #15) — enables direct JSON / serde output of gathered device facts.

### Changed

- Bumped `rustnetconf` dependency to `0.9`.

[0.9.0]: https://github.com/fastrevmd-lab/rustEZ/releases/tag/v0.9.0
