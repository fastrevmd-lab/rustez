# Changelog

All notable changes to the `rustez` crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.13.0] — 2026-07-19

### Fixed

- **`memory_total` is no longer null on vSRX route engines** (#30). vSRX emits
  the total as `<memory-system-total>`, which the parser did not recognize, so
  the value fell through silently. The element is now parsed. Note it arrives
  as a **bare number** (`16323`) where MX/RE-VMX emits a unit-bearing string
  (`<memory-dram-size>4096 MB</memory-dram-size>`); see *Changed* below for how
  this is reconciled.
- **`master_re` is no longer null on standalone devices** (#30). Platforms such
  as vSRX omit `<mastership-state>` entirely, which left `find_master_re`
  returning `None` on every single-RE chassis. A lone RE reporting no mastership
  state is now treated as the master. A lone RE that *explicitly* reports a
  non-master state is left alone — the device's own answer wins over the
  inference — and a multi-RE chassis reporting no state anywhere still yields
  `None` rather than a guess.

  The `RouteEngine.mastership_state` field itself is **not** synthesized: it
  stays `None` on these platforms, reflecting what the device actually said.
  Only the derived `master_re` changes.

### Changed

- **`RouteEngine.memory_total` is normalized to a `"N MB"` string.** Values
  parsed from `<memory-system-total>` gain an explicit ` MB` suffix so the
  field has one shape across platforms rather than exposing per-platform
  formatting to callers. Values that already carry a unit (the existing
  `memory-dram-size` / `memory-installed-size` path) pass through untouched and
  are never double-suffixed. **Callers that string-match `memory_total` exactly
  may need updating**; callers that parse a leading integer are unaffected.
- Bumped `rustnetconf` dependency to `0.13`. No source changes were required —
  the release's `DeviceConfig.vendor` `Box` → `Arc` break does not touch any API
  rustEZ uses.

### Notes

- `RouteEngine.status` of `"Testing"` on vSRX is **genuine device output**, not a
  parse artifact, and is deliberately passed through unmodified.
- The vSRX fixture backing these fixes is a verbatim capture from Junos
  24.4R1.9.

## [0.12.1] — 2026-07-02

### Security

- **Upgraded `quick-xml` `0.37` → `0.41`** — closes **RUSTSEC-2026-0194**
  (quadratic duplicate-attribute-name scan) and **RUSTSEC-2026-0195**
  (unbounded namespace-declaration allocation / memory-exhaustion DoS). Both
  are reachable on the fact-parsing path, which decodes device-supplied XML.

### Fixed

- **Fact parsers no longer truncate values containing XML entities.** Since
  quick-xml 0.38, entity references (`&amp;`, `&lt;`, `&#38;`, …) stream as
  separate `Event::GeneralRef` events instead of arriving inside `Text`. The
  four fact-parser reader loops (`facts/mod.rs`, `chassis.rs`, `software.rs`,
  `routing_engine.rs`) now accumulate `Text` + resolve `GeneralRef` and flush
  on the closing tag, so a Junos value such as a description or config
  fragment containing `&`/`<`/`>` round-trips correctly. Added entity
  round-trip regression tests. `unwrap_multi_re` keeps entities verbatim in
  reconstructed per-RE XML (and now escapes reconstructed attribute-value
  quotes) so downstream re-parsing stays well-formed.

### Changed

- Bumped `rustnetconf` dependency to `0.12.3` (pulls its own quick-xml 0.41
  fix for the same advisories).
- **MSRV raised to 1.79** (required by quick-xml ≥ 0.40).

## [0.12.0] — 2026-05-18

### Added

- **`HostKeyVerification::KnownHosts(PathBuf)` re-exported** — surfaces the new
  variant from `rustnetconf 0.12`. Callers can now point at an OpenSSH
  `known_hosts` file instead of pinning a single fingerprint.
- **Python `Device(host_key_known_hosts="...")`** — new constructor keyword
  argument on `rustez.Device` mapping to `HostKeyVerification::KnownHosts`.
  Mutually exclusive with `host_key_fingerprint` (raises `ValueError`).

### Changed

- Bumped `rustnetconf` dependency to `0.12`. Additive only for rustez — no
  source-level breakage. The 0.12 release added `KnownHosts(PathBuf)` to
  `HostKeyVerification` and a `host_key_verification` field on the pool
  `DeviceConfig` struct (rustez does not use the pool API).

### Fixed

- **Stale doc comments** on `DeviceBuilder::host_key_verification` (Rust) and
  `Device.__init__` (Python) — both incorrectly claimed the default policy
  was `AcceptAll`. Since `rustnetconf 0.11` the default has been `RejectAll`
  (fail-closed); the docs now reflect this.

[0.13.0]: https://github.com/fastrevmd-lab/rustez/compare/v0.12.1...v0.13.0
[0.12.1]: https://github.com/fastrevmd-lab/rustez/compare/v0.12.0...v0.12.1
[0.12.0]: https://github.com/fastrevmd-lab/rustez/compare/v0.11.0...v0.12.0

## [0.11.0] — 2026-05-18

### Changed

- **BREAKING:** Bumped `rustnetconf` dependency to `0.11`. The underlying SSH host-key verification default changed from `AcceptAll` to `RejectAll` (fail-closed). Callers that previously connected without setting a policy will now get a host-key rejection error at connect time.
  - **Migration:** Pin a fingerprint with `DeviceBuilder::host_key_verification(HostKeyVerification::Fingerprint(...))` (recommended), or explicitly opt back into the old behavior with `DeviceBuilder::host_key_verification(HostKeyVerification::AcceptAll)` for lab/test use.
  - **Python:** Pass `host_key_fingerprint="..."` to `Device(...)` to pin, or use `HostKeyVerification` directly via the native bindings.
- Integration test harness (`vsrx_builder` in `tests/integration_vsrx.rs`) updated to explicitly request `HostKeyVerification::AcceptAll` since the lab vSRX devices are known-good.

[0.11.0]: https://github.com/fastrevmd-lab/rustez/compare/v0.10.1...v0.11.0

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

[0.10.0]: https://github.com/fastrevmd-lab/rustez/compare/v0.9.0...v0.10.0

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

[0.9.0]: https://github.com/fastrevmd-lab/rustez/releases/tag/v0.9.0
