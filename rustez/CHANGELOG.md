# Changelog

All notable changes to the `rustez` crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
