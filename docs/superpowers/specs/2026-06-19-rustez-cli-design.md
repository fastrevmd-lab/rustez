# rustez-cli Implementation Design

**Issue:** fastrevmd-lab/rustEZ#20 — Implement rustez-cli beyond placeholder binary
**Date:** 2026-06-19
**Status:** Approved (brainstorming)

## Problem

`rustez-cli` is advertised and packaged as the command-line interface for rustEZ, but the
binary is a `Hello, world!` placeholder. A downstream project wants to replace a local PyEZ
bridge by shelling out to a RustEZ binary, which is impractical today because the CLI has no
commands, argument parsing, output schema, or exit-code/error contract.

## Goals

- Implement the README-documented commands as a usable human CLI **and** a stable
  machine-readable interface for bridge/integration use — both first-class (no divergence:
  one typed result model rendered as either text or JSON).
- Stable JSON envelope and per-category exit codes so a bridge can parse one shape and branch
  on `ok` / `error.kind`.
- Secure-by-default SSH host-key handling and credential input that avoids leaking secrets via
  shell history / process list.

## Non-Goals (YAGNI)

- Pushing command logic into the `rustez` core library (the library already exposes the needed
  primitives; keep it untouched).
- Mocking a full NETCONF server for end-to-end command tests.
- Shell-completion generation; config-file/profile support; `--password-stdin`/`--password-file`.

## Approach

Modular CLI crate (no library changes). Each command is built around a typed result struct that
renders to either human text or a JSON envelope. Error classification and exit-code mapping live
in the CLI, doing best-effort inspection of `RustEzError` / `NetconfError` variants.

## Module Layout

```
rustez-cli/src/
  main.rs        # thin: init tracing, parse args, dispatch, print envelope, set exit code
  cli.rs         # clap derive: Cli { Command enum }, flattened ConnOpts
  connect.rs     # build_device(&ConnOpts) -> Result<Device, CliError>
  output.rs      # Envelope<T>, CommandData enum, json + text rendering
  error.rs       # CliError { kind, message }, ErrorKind::exit_code(), classify(RustEzError, ctx)
  commands/
    mod.rs
    facts.rs     # -> FactsData (library Facts)
    rpc.rs       # -> RpcData
    config.rs    # apply / diff / commit-check / commit / confirm / rollback -> ConfigData
```

### Data flow per invocation

1. `main` parses `Cli`. clap parse error → exit 1 (`usage`), envelope/message on stderr.
2. Dispatch to a command handler. Handler calls `connect::build_device()`, runs library calls,
   returns `Result<CommandData, CliError>`.
3. `main` wraps the result in an `Envelope` (`ok:true` + data, or `ok:false` + error), renders
   as JSON (if `--json`) or text, prints, exits with `error.exit_code()` (0 on success).
4. Device is always `close()`d (best-effort) before returning.

Each invocation = one connect → operate → close cycle (one NETCONF session). This is what makes
the per-invocation confirming-commit model (below) work.

## Command Grammar (clap)

### Shared connection options (`ConnOpts`, flattened into every command)

```
<HOST>                          positional, required
-u, --user <USER>               required
-p, --password <PW>             optional; emits stderr warning when used
    --port <PORT>               optional (library default otherwise)
    --key-file <PATH>           optional; key-based auth
    --host-key-fingerprint <FP>     \
    --known-hosts <PATH>             } mutually exclusive (clap group); all optional
    --accept-any-host-key            /  none set => RejectAll (fail-closed)
    --timeout <SECS>            optional; maps to DeviceBuilder::rpc_timeout
    --json                      machine-readable envelope output
```

### Commands

```
rustez facts <HOST> [conn]
rustez rpc   <HOST> <COMMAND> [--format text|xml] [conn]
rustez config apply        <HOST> -f <FILE> [--format set|text|xml] [conn]   # default set
rustez config diff         <HOST> -f <FILE> [--format set|text|xml] [conn]   # load file, show candidate diff
rustez config commit-check <HOST> -f <FILE> [--format ...] [conn]
rustez config commit       <HOST> -f <FILE> [--format ...] [--confirm-minutes <N>] [--comment <TEXT>] [conn]
rustez config confirm      <HOST> [conn]                                     # bare confirming commit
rustez config rollback     <HOST> [--id <N>] [conn]                          # default id 0
```

**Command semantics:**

- `facts` — gather + return device facts.
- `rpc <COMMAND>` — run an operational CLI command via `Device::cli()`. `--format text|xml`
  (default text). Output is device-formatted text wrapped as a string in JSON mode.
- `config apply` — load file + commit (the simple convenience verb; matches README).
- `config commit` — load file + commit, exposing commit options (`--confirm-minutes`,
  `--comment`). Overlaps `apply` intentionally; both kept (`apply` beginner-friendly &
  README-accurate, `commit` the option-bearing form).
- `config commit-check` — load file + validate (`commit_check`) without committing.
- `config diff` — load file, return candidate-vs-active diff.
- `config confirm` — bare confirming commit (no file). Confirms a prior
  `commit --confirm-minutes N` from a separate invocation/session before the rollback timer
  fires (Junos allows the confirming commit from a different session).
- `config rollback [--id N]` — roll back to rollback id N (default 0).
- `--format` maps to `ConfigPayload::Set` / `Text` / `Xml`. Default `set` (matches README's
  `config.set` example). `xml` is documented as injected unescaped — untrusted-input caveat.

### commit-confirmed flow (two invocations)

```bash
rustez config commit 10.0.0.1 --confirm-minutes 5 -f new.conf   # arms the auto-rollback timer
rustez config confirm 10.0.0.1                                  # confirming commit before timer fires
```

Maps to `ConfigManager::commit_confirmed(seconds)` then a bare `ConfigManager::commit()`.

## JSON Envelope & Output Model

Stable envelope, identical across every command:

```json
{
  "ok": true,
  "command": "facts",
  "host": "10.0.0.1",
  "data": { },
  "error": null
}
```

On failure: `{"ok": false, "command": ..., "host": ..., "data": null, "error": {"kind": "...", "message": "..."}}`.

Implemented as a serde `Serialize` struct generic over the payload; `data` holds a `CommandData`
enum flattening to the per-command shape:

- **facts** → library `Facts` directly: `{hostname, model, version, serial_number, personality,
  route_engines, master_re, domain, fqdn, is_cluster}`.
- **rpc** → `{"output": "<device text>", "format": "text"|"xml"}`.
- **config apply / commit** → `{"loaded": true, "committed": true, "confirm_minutes": 5|null, "warnings": [...]}`.
- **config diff** → `{"diff": "<text>"|null}`.
- **config commit-check** → `{"loaded": true, "check_passed": true, "warnings": [...]}`.
- **config confirm** → `{"committed": true}`.
- **config rollback** → `{"rolled_back": true, "id": 0}`.

**Text mode** (no `--json`): `facts` as an aligned key/value block; `rpc` prints raw device
output verbatim; `diff` prints the diff verbatim; commit/rollback/confirm print a one-line
confirmation. Warnings (from `load_with_warnings`) print to stderr in text mode and populate the
`warnings` array in JSON mode.

**Streams:** success (envelope/text) → stdout; failure (JSON envelope or text error line) →
stderr. A bridge can capture stdout for data and treat stderr + nonzero exit as failure.

## Error Taxonomy & Exit Codes

`CliError { kind: ErrorKind, message: String }`; `ErrorKind` carries the exit code:

| Code | kind       | Trigger |
|------|------------|---------|
| 0    | —          | success |
| 1    | `usage`    | clap parse error / mutually-exclusive host-key flags / missing file / no credential source on non-TTY |
| 2    | `connect`  | TCP/SSH connect failure, timeout at connect, host-key rejection |
| 3    | `auth`     | authentication failure |
| 4    | `rpc`      | operational RPC/CLI command error |
| 5    | `load`     | config load rejected / parse error from device |
| 6    | `commit`   | commit, commit-check, confirm failure |
| 7    | `rollback` | rollback failure |
| 8    | `internal` | unmapped / unexpected error |

### Classifier `classify(RustEzError, context) -> ErrorKind`

Best-effort, library untouched. `context` identifies the command/phase so the same `Netconf`
error maps to the right category.

- `RustEzError::Netconf(e)` → inspect inner `NetconfError`: transport/connect/host-key variants →
  `connect`; auth variants → `auth`; otherwise fall back by context (load op → `load`, commit op →
  `commit`, etc.).
- `RustEzError::Timeout(_)` → `connect` if during connect, else the context kind.
- `RustEzError::Config(_)` → `load` or `commit` per the operation.
- `RustEzError::Rpc(_)` / `Facts(_)` → `rpc`.
- `RustEzError::NotConnected` / `XmlParse(_)` / other → `internal`.

Exact `NetconfError` variant names are confirmed against the dependency at implementation time,
not guessed in this spec.

## Connection, Credentials & Host-Key Handling

Single `connect::build_device(&ConnOpts) -> Result<Device, CliError>` used by all commands.

### Credential resolution (precedence high → low)

1. `-p/--password <PW>` → also write a one-line stderr warning (visible in process list; prefer
   `$RUSTEZ_PASSWORD`).
2. else `$RUSTEZ_PASSWORD`.
3. else `--key-file` → key-based auth, no password needed.
4. else stdin is a TTY → interactive no-echo prompt.
5. else → `CliError{kind: usage, "no password provided and not a TTY; set $RUSTEZ_PASSWORD or use --key-file"}`.

### Host-key mapping (clap mutually-exclusive group)

- `--host-key-fingerprint <fp>` → `HostKeyVerification::Fingerprint(fp)`
- `--known-hosts <path>` → `HostKeyVerification::KnownHosts(path.into())`
- `--accept-any-host-key` → `HostKeyVerification::AcceptAll`
- none → policy unset → library default `RejectAll` → connect fails with a `connect`-kind error
  whose message names the three flags.

### Builder assembly

`Device::connect(host).username(u)`, then conditionally `.password()` / `.key_file()` / `.port()`
/ `.rpc_timeout(timeout)` / `.host_key_verification(policy)`, then `.open()`. Facts auto-gather
**on** only for the `facts` command; `rpc` and `config` commands use `.no_facts()` to save 3 RPCs
per invocation.

## Testing

Following repo conventions (canned-data unit tests; integration gated behind `#[ignore]` + env vars).

**Unit (no device):**
- **Arg parsing** — `Cli::try_parse_from(...)` over representative argv: required/optional args,
  `--format` validation, host-key mutually-exclusive group rejecting two flags, `--json`.
- **Error classifier** — constructed `RustEzError` + context → assert `ErrorKind` / exit code
  across connect/auth/load/commit/rpc/internal.
- **Output rendering** — build each `CommandData`, serialize `Envelope`, assert JSON shape
  (`ok`, `command`, `host`, `data`, `error`); assert text renderer for `facts` and a one-line
  commit confirmation.
- **Credential resolution** — `resolve_password()` with mocked env/flag/no-tty inputs asserting
  precedence and the step-5 usage error (TTY prompt path skipped).

**Integration (one, `#[ignore]`, vSRX):** `rustez facts` against the lab vSRX via
`RUSTEZ_VSRX_HOST/USER/KEY`, asserting exit 0 and a parseable JSON envelope with non-empty
`hostname`. Uses `--accept-any-host-key` like the existing harness.

## README Follow-up

README CLI examples remain accurate (`facts`, `rpc`, `config apply`). Add `--json` and host-key
flag usage notes when the CLI lands.
