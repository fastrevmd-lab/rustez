# rustEZ code review and security audit

Date: 2026-05-18T11:53:47-04:00
Repository: `/home/mharman/rustEZ`

## Scope

Reviewed the rustEZ workspace:

- `rustez` core Rust library
- `rustez-py` PyO3 native extension and Python compatibility wrapper
- `rustez-cli`
- GitHub Actions CI and PyPI publishing workflows
- Rust and Python dependency manifests/lockfiles

Excluded generated/build output such as `.git/`, `target/`, and package artifacts under `target/package/`.

## Method

Manual source review plus automated checks:

- `cargo check --workspace` — passed
- `cargo test --workspace` — passed; 46 rustez unit tests passed, 5 integration tests ignored, 3 doctests passed
- `cargo clippy --workspace --all-targets -- -D warnings` — passed
- `cargo audit --json` — found one advisory: `RUSTSEC-2023-0071` for `rsa 0.10.0-rc.16`
- `pip-audit` against `rustez-py/pyproject.toml` dependencies — no known vulnerabilities found
- `python3 -m pytest rustez-py/tests -q` — passed; 4 tests passed
- High-signal source searches for secrets, injection sinks, shell execution, XML construction, host-key verification, and operational cleanup paths
- Independent reviewer subagent pass over SSH host-key verification, ProxyCommand handling, XML handling, config cleanup, Python wrapper behavior, and CI/publishing

## Executive summary

The codebase is small, readable, and has useful defensive controls around XML name validation and XML escaping for most generated RPC payloads. The main security concern is SSH trust: rustEZ does not expose target-device host-key verification, so users inherit rustnetconf's insecure `AcceptAll` default for the final NETCONF SSH target. CI also explicitly ignores a known `rsa` cryptographic advisory, which should be treated as a tracked risk rather than a permanently suppressed gate.

No hardcoded production secrets were found. The core Rust unit tests and clippy checks pass. The Python package dependency scan found no known vulnerabilities.

## Findings

### RZ-SEC-001: Target device SSH host-key verification is not exposed, so users inherit insecure AcceptAll behavior

Severity: High
Status: Confirmed by source review

Affected files:

- `rustez/Cargo.toml:13-15`
- `rustez/src/device.rs:343-455`
- Dependency evidence: `rustnetconf-0.10.0/src/client.rs:133-159`, `rustnetconf-0.10.0/src/client.rs:338-356`, `rustnetconf-0.10.0/src/transport/ssh.rs:62-79`

Evidence:

```toml
# rustez/Cargo.toml
[dependencies]
rustnetconf = "0.10"
```

```rust
// rustez/src/device.rs
pub struct DeviceBuilder {
    host: String,
    port: Option<u16>,
    username: Option<String>,
    password: Option<String>,
    key_file: Option<String>,
    gather_facts: bool,
    rpc_timeout: Option<Duration>,
    keepalive_interval: Option<Duration>,
    jump_hosts: Vec<JumpHostConfig>,
    proxy_command: Option<String>,
}
```

```rust
// rustez/src/device.rs
let mut builder = Client::connect(&address);

if let Some(ref username) = self.username {
    builder = builder.username(username);
}
if let Some(ref password) = self.password {
    builder = builder.password(password);
}
if let Some(ref key_file) = self.key_file {
    builder = builder.key_file(key_file);
}
...
let mut client = builder.connect().await?;
```

The local `rustnetconf 0.10.0` dependency documents and initializes the default as `HostKeyVerification::AcceptAll`:

```rust
// rustnetconf-0.10.0/src/client.rs
/// Default: [`HostKeyVerification::AcceptAll`] (a warning is logged).
pub fn host_key_verification(mut self, policy: HostKeyVerification) -> Self {
    self.host_key_verification = policy;
    self
}
...
host_key_verification: HostKeyVerification::AcceptAll,
```

Impact:

An attacker able to intercept or redirect the NETCONF SSH connection can present an arbitrary host key and rustEZ callers have no rustEZ API to require a known fingerprint or known-hosts-style verification for the final target. This weakens confidentiality and integrity for device automation, including configuration changes and credential use.

Recommended remediation:

1. Add a `host_key_verification` field to `DeviceBuilder`.
2. Expose a Rust builder method, e.g. `DeviceBuilder::host_key_verification(HostKeyVerification)`.
3. Forward the policy to `rustnetconf::ClientBuilder::host_key_verification(...)` in `DeviceBuilder::open()`.
4. Expose equivalent Python parameters for fingerprint pinning and/or a known-hosts policy.
5. Consider making secure verification the documented production path and reserving AcceptAll for explicit lab/test use.
6. Add unit tests that assert the policy is stored and forwarded, plus Python wrapper tests for the new option.

Agent-ready remediation prompt:

```text
Fix RZ-SEC-001 in /home/mharman/rustEZ. Add target SSH host-key verification support to rustez::DeviceBuilder and the rustez-py Device wrapper. Forward the selected rustnetconf::transport::ssh::HostKeyVerification policy into Client::connect(...).host_key_verification(...). Preserve current behavior only when the caller explicitly opts into insecure AcceptAll or when maintaining backward compatibility requires a default; document the security implications. Add Rust unit tests and Python API tests. Verify with: cargo test --workspace; cargo clippy --workspace --all-targets -- -D warnings; python3 -m pytest rustez-py/tests -q.
```

### RZ-SEC-002: CI ignores known `rsa` Marvin timing-side-channel advisory

Severity: Medium
Status: Confirmed by automated scan and source review

Affected files:

- `.github/workflows/ci.yml:37-41`
- `Cargo.lock:1798-1814`
- `Cargo.lock:1998-2011`

Evidence:

```yaml
# .github/workflows/ci.yml
- name: cargo audit
  run: cargo audit --ignore RUSTSEC-2023-0071
```

```text
cargo audit:
RUSTSEC-2023-0071 rsa 0.10.0-rc.16
Marvin Attack: potential key recovery through timing sidechannels
https://github.com/RustCrypto/RSA/issues/626
CVSS:3.1/AV:N/AC:H/PR:N/UI:N/S:U/C:H/I:N/A:N
```

```toml
# Cargo.lock
[[package]]
name = "rsa"
version = "0.10.0-rc.16"
...
[[package]]
name = "rustnetconf"
version = "0.10.0"
dependencies = [
 "russh",
 ...
]
```

Impact:

The current CI security gate remains green while a known cryptographic advisory is present in the lockfile. Even if exploitability depends on whether RSA private-key operations are reachable in the SSH dependency stack, a permanent ignore can hide future risk and makes release security posture unclear.

Recommended remediation:

1. Open or link a tracked issue for the advisory with reachability analysis.
2. Prefer upgrading `rustnetconf`/`russh` or transitive dependencies once a non-vulnerable path is available.
3. If the advisory must remain ignored temporarily, add an expiry date and comment explaining reachability, risk acceptance, and the dependency path.
4. Consider a CI step that prints ignored advisories so they remain visible in logs.

Agent-ready remediation prompt:

```text
Fix or formally track RZ-SEC-002 in /home/mharman/rustEZ. Investigate the cargo-audit RUSTSEC-2023-0071 advisory for rsa 0.10.0-rc.16 in the rustnetconf/russh dependency path. Prefer dependency updates that remove the advisory. If no patched version is available, update .github/workflows/ci.yml to document the temporary ignore with a tracking issue, expiry/review date, and reachable-risk rationale. Verify with cargo audit, cargo test --workspace, and cargo clippy --workspace --all-targets -- -D warnings.
```

### RZ-SEC-003: Auto-opened Junos private configuration database is not automatically closed if load fails

Severity: Medium
Status: Likely; no automatic cleanup found on failing load paths

Affected files:

- `rustez/src/config.rs:78-87`
- `rustez/src/config.rs:94-107`
- `rustez/src/config.rs:114-122`
- `rustez/src/config.rs:205-211`

Evidence:

```rust
// rustez/src/config.rs
pub async fn load(&mut self, payload: ConfigPayload) -> Result<String, RustEzError> {
    self.auto_open_if_needed().await?;

    let (action, format, config) = payload_to_load_args(&payload);
    let timeout = self.timeout;
    timed(
        timeout,
        self.client.load_configuration(action, format, &config),
    )
    .await
}
```

```rust
// rustez/src/config.rs
async fn auto_open_if_needed(&mut self) -> Result<(), RustEzError> {
    if self.client.requires_open_configuration() && !*self.config_db_open {
        self.open_configuration(OpenConfigurationMode::Private)
            .await?;
    }
    Ok(())
}
```

`unlock()` closes an open private/exclusive configuration database before unlocking, but callers must still reach and call `unlock()` after a failed load. The load functions themselves do not close an auto-opened database when `load_configuration`, `load_with_action`, or `rpc_with_warnings` returns an error or times out.

Impact:

On clustered devices where `auto_open_if_needed()` opens a private database, malformed config or device-side load errors can leave the session in an open configuration state. That can surprise subsequent operations in the same process and may contribute to stale operational state or lock/cleanup problems, especially in automation that returns immediately on error.

Recommended remediation:

1. Track whether `load*()` opened the configuration database during this call.
2. On load error or timeout, best-effort close that database and reset `config_db_open` if close succeeds.
3. Preserve explicit user-opened configuration sessions; do not close a database opened before the load call.
4. Add tests around `auto_open_if_needed()` error paths using a mock/fake client if feasible, or integration tests against vSRX where practical.

Agent-ready remediation prompt:

```text
Fix RZ-SEC-003 in /home/mharman/rustEZ. In ConfigManager::load, load_with_action, and load_with_warnings, detect whether auto_open_if_needed opened the private configuration database for this call. If the subsequent load operation fails or times out, best-effort close the auto-opened configuration database and update config_db_open safely. Do not close sessions that the caller opened explicitly before the load. Add regression tests if possible, and verify with cargo test -p rustez and cargo clippy -p rustez --all-targets -- -D warnings.
```

### RZ-QUAL-001: Rust and Python package versions are inconsistent

Severity: Low
Status: Confirmed by source review

Affected files:

- `rustez/Cargo.toml:1-4`
- `rustez-py/Cargo.toml:1-4`
- `rustez-py/pyproject.toml:5-8`

Evidence:

```toml
# rustez/Cargo.toml
name = "rustez"
version = "0.10.1"
```

```toml
# rustez-py/Cargo.toml
name = "rustez-py"
version = "0.10.0"
```

```toml
# rustez-py/pyproject.toml
name = "rustez"
version = "0.10.0"
```

Impact:

Release and vulnerability triage become harder because the Python wheel can embed/use a newer core crate while advertising an older Python package version. This can confuse users, support, and security scanners.

Recommended remediation:

1. Decide whether all workspace crates should share a version.
2. If yes, update `rustez-py/Cargo.toml` and `rustez-py/pyproject.toml` to match `rustez`.
3. Add a CI check that fails on version drift.

Agent-ready remediation prompt:

```text
Fix RZ-QUAL-001 in /home/mharman/rustEZ. Align rustez, rustez-py, and pyproject package versions or add a documented policy explaining intentional divergence. Add a lightweight CI/script check that catches accidental version drift before publishing. Verify with cargo check --workspace and python3 -m pytest rustez-py/tests -q.
```

### RZ-QUAL-002: Python `Config.diff(rb_id=...)` accepts a rollback ID but the native binding ignores it

Severity: Low
Status: Confirmed by source review

Affected files:

- `rustez-py/python/rustez/__init__.py:480-493`
- `rustez-py/src/lib.rs:395-407`
- `rustez/src/config.rs:125-137`

Evidence:

```python
# rustez-py/python/rustez/__init__.py
def diff(self, rb_id: int = 0) -> str | None:
    result = self._native.config_diff(rb_id=rb_id)
    return result if result else None
```

```rust
// rustez-py/src/lib.rs
#[pyo3(signature = (rb_id=None))]
fn config_diff(&self, py: Python<'_>, rb_id: Option<u32>) -> PyResult<String> {
    let _ = rb_id; // reserved for future rollback-id support
    ...
    let result = self.runtime.block_on(cfg.diff()).map_err(to_py_err)?;
```

```rust
// rustez/src/config.rs
pub async fn diff(&mut self) -> Result<Option<String>, RustEzError> {
    let timeout = self.timeout;
    let response: String = timed(timeout, self.client.get_configuration_compare(0)).await?;
```

Impact:

Callers can request a rollback comparison, but the implementation always compares against rollback ID 0/running. This can cause automation or review tooling to make decisions using the wrong diff.

Recommended remediation:

1. Add `ConfigManager::diff_against(rb_id: u32)` or change `diff()` to accept an optional rollback ID.
2. Forward the Python `rb_id` into the Rust call.
3. Add tests asserting non-default rollback IDs reach `get_configuration_compare(rb_id)`.

Agent-ready remediation prompt:

```text
Fix RZ-QUAL-002 in /home/mharman/rustEZ. Implement rollback-ID-aware config diffs. The Python Config.diff(rb_id=N) argument must be forwarded through rustez-py into rustez::ConfigManager and ultimately to client.get_configuration_compare(N), while preserving current default rb_id=0 behavior. Add tests for rb_id forwarding and verify with cargo test --workspace and python3 -m pytest rustez-py/tests -q.
```

## Positive controls observed

- RPC name and argument keys are validated before XML element construction: `rustez/src/rpc.rs:97-115`, `rustez/src/rpc.rs:123-140`.
- RPC argument values are XML-escaped: `rustez/src/rpc.rs:109-112`.
- CLI command text is XML-escaped and the `format` attribute is validated as an XML name: `rustez/src/rpc.rs:82-87`.
- Config `Text` and `Set` payloads are escaped in the raw warning-path XML builder: `rustez/src/config.rs:245-263`.
- Raw XML config payloads are explicitly documented as trusted-only: `rustez/src/config.rs:29-34`.
- Commit comments are XML-escaped: `rustez/src/config.rs:154-157`, `rustez/src/config.rs:270-272`.
- The Python `get_config` wrapper validates `format` against a fixed allowlist and parses `filter_xml` before embedding it: `rustez-py/python/rustez/__init__.py:128-151`.
- GitHub Actions use minimal `contents: read` permission for CI and OIDC-only PyPI publishing permissions in the publish job: `.github/workflows/ci.yml:8-9`, `.github/workflows/publish-pypi.yml:49-67`.
- No hardcoded production credentials or API tokens were found. The `passwd="secret"` occurrences are documentation examples, not runtime secrets.

## Dependency scan summary

Rust:

- `cargo audit --json` found one advisory:
  - `RUSTSEC-2023-0071` — `rsa 0.10.0-rc.16`, Marvin Attack timing side channel, CVSS `CVSS:3.1/AV:N/AC:H/PR:N/UI:N/S:U/C:H/I:N/A:N`, no patched version reported by cargo-audit.
- `Cargo.lock` includes `rustnetconf 0.10.0`, which depends on `russh 0.60.2`; `rsa 0.10.0-rc.16` is present in the lockfile.
- `cargo tree --workspace --target all -i rsa` printed no reverse tree in this environment despite the lockfile and cargo-audit finding. Treat reachability as requiring follow-up analysis, not as proven unreachable.

Python:

- `pip-audit` for `rustez-py/pyproject.toml` dependencies found no known vulnerabilities.
- The only runtime Python dependency declared is `lxml>=4.9.0`.

## Recommended remediation order

1. RZ-SEC-001 — expose and document secure SSH host-key verification for final targets.
2. RZ-SEC-002 — remove or formally track the `cargo audit` ignore for `RUSTSEC-2023-0071`.
3. RZ-SEC-003 — add best-effort cleanup for auto-opened configuration databases on load failure.
4. RZ-QUAL-002 — fix rollback-ID-aware diffs to avoid misleading automation output.
5. RZ-QUAL-001 — align package versions or add a version-drift policy/check.

## Limitations

- Integration tests were not run because they require a reachable vSRX and environment variables (`RUSTEZ_VSRX_HOST`, `RUSTEZ_VSRX_USER`, and key/password auth).
- Dynamic MITM, SSH host-key, ProxyJump, and ProxyCommand behavior was not tested against live network devices.
- The review did not include fuzzing of XML parsers or Junos RPC responses.
- `cargo tree -i rsa` did not show a reverse dependency path even though `cargo audit` and `Cargo.lock` show the advisory/package. Reachability should be investigated before assigning exploitability for RZ-SEC-002.
