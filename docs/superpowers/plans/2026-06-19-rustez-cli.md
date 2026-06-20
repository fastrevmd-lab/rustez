# rustez-cli Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the `rustez-cli` placeholder binary with a real CLI exposing facts/rpc/config commands, a stable JSON envelope, and a per-category exit-code contract for bridge/integration use.

**Architecture:** Modular CLI crate (no `rustez` library changes). clap derive defines the grammar; each command handler connects via a shared `build_device`, calls the library, and returns a typed `CommandData`. `main` wraps the result in an `Envelope` rendered as JSON (`--json`) or human text, and maps errors to exit codes via a `classify()` function that inspects `RustEzError`/`NetconfError` variants.

**Tech Stack:** Rust (edition 2021, requires rustc ≥ 1.70 for `std::io::IsTerminal`), `clap` 4 (derive), `serde`/`serde_json`, `rpassword` (no-echo prompt), `tokio`, `rustez` (path dep).

**Spec:** `docs/superpowers/specs/2026-06-19-rustez-cli-design.md`

---

## File Structure

```
rustez-cli/
  Cargo.toml          # add serde(derive) + rpassword deps
  src/
    main.rs           # parse, dispatch, render envelope, set exit code
    cli.rs            # clap derive: Cli, Command, ConnOpts, per-command Args, ValueEnums
    error.rs          # ErrorKind, CliError, Phase, classify(), from_rustez()
    output.rs         # Envelope, ErrorBody, CommandData, render_text(), warnings()
    connect.rs        # plan_password(), host_key_policy(), build_device()
    commands/
      mod.rs          # re-exports
      facts.rs        # facts handler
      rpc.rs          # rpc handler
      config.rs       # apply/diff/commit-check/commit/confirm/rollback handlers
  tests/
    cli_integration.rs  # one #[ignore] vSRX test
```

**Dependency order between modules:** `error.rs` → `cli.rs` → `output.rs` → `connect.rs` → `commands/*` → `main.rs`. Build in that order so each task compiles on its own.

---

## Task 1: Add crate dependencies

**Files:**
- Modify: `rustez-cli/Cargo.toml`

- [ ] **Step 1: Add serde derive and rpassword to dependencies**

Replace the `[dependencies]` section of `rustez-cli/Cargo.toml` with:

```toml
[dependencies]
rustez = { path = "../rustez" }
tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rpassword = "7"
```

- [ ] **Step 2: Verify it resolves**

Run: `cargo check -p rustez-cli`
Expected: compiles (the placeholder `main.rs` still builds). New deps download/resolve without error.

- [ ] **Step 3: Commit**

```bash
git add rustez-cli/Cargo.toml Cargo.lock
git commit -m "chore(cli): add serde-derive and rpassword deps"
```

---

## Task 2: Error taxonomy, phases, and classifier (`error.rs`)

**Files:**
- Create: `rustez-cli/src/error.rs`

This is the keystone: the exit-code contract and the `RustEzError` → `ErrorKind` mapping. The exact `NetconfError` variants used below were confirmed against `rustnetconf 0.12.0` (`TransportError::{Connect,Auth,Channel,ChannelClosed,Io,Ssh,HostKeyMismatch,HostKeyNotInKnownHosts,HostKeyRevoked}`, `RpcError::{ServerError,Timeout,CommitUnknown,ParseError,MessageIdMismatch}`, `ProtocolError::{CapabilityMissing,SessionClosed,SessionExpired,HelloFailed,Xml}`).

- [ ] **Step 1: Write the failing tests**

Create `rustez-cli/src/error.rs` with the tests first (module + types stubbed enough to compile will come in Step 3; write tests now and let them fail to compile):

```rust
//! CLI error taxonomy, command phases, and the RustEzError classifier.

use rustez::RustEzError;

/// Failure category. The discriminant maps directly to the process exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Usage,
    Connect,
    Auth,
    Rpc,
    Load,
    Commit,
    Rollback,
    Internal,
}

impl ErrorKind {
    /// Process exit code for this error category (0 is reserved for success).
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorKind::Usage => 1,
            ErrorKind::Connect => 2,
            ErrorKind::Auth => 3,
            ErrorKind::Rpc => 4,
            ErrorKind::Load => 5,
            ErrorKind::Commit => 6,
            ErrorKind::Rollback => 7,
            ErrorKind::Internal => 8,
        }
    }

    /// Stable lowercase string used in the JSON envelope `error.kind` field.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Usage => "usage",
            ErrorKind::Connect => "connect",
            ErrorKind::Auth => "auth",
            ErrorKind::Rpc => "rpc",
            ErrorKind::Load => "load",
            ErrorKind::Commit => "commit",
            ErrorKind::Rollback => "rollback",
            ErrorKind::Internal => "internal",
        }
    }
}

/// The command phase a library call ran in, so the classifier can map an
/// otherwise-ambiguous device error (e.g. a generic RPC server error) to the
/// right category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Connect,
    Facts,
    Rpc,
    Load,
    Commit,
    Rollback,
}

/// A CLI-level error carrying its category and a human message.
#[derive(Debug)]
pub struct CliError {
    pub kind: ErrorKind,
    pub message: String,
}

impl CliError {
    /// Construct a CLI error with an explicit category.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        CliError {
            kind,
            message: message.into(),
        }
    }

    /// Classify a library error in the context of the phase that produced it.
    pub fn from_rustez(err: &RustEzError, phase: Phase) -> Self {
        CliError {
            kind: classify(err, phase),
            message: err.to_string(),
        }
    }
}

/// Default category for an error that is only distinguishable by phase.
fn kind_for_phase(phase: Phase) -> ErrorKind {
    match phase {
        Phase::Connect => ErrorKind::Connect,
        Phase::Facts | Phase::Rpc => ErrorKind::Rpc,
        Phase::Load => ErrorKind::Load,
        Phase::Commit => ErrorKind::Commit,
        Phase::Rollback => ErrorKind::Rollback,
    }
}

/// Map a `RustEzError` to an `ErrorKind`, using `phase` as a tiebreaker.
pub fn classify(err: &RustEzError, phase: Phase) -> ErrorKind {
    use rustnetconf::error::{NetconfError, ProtocolError, RpcError, TransportError};

    match err {
        RustEzError::Netconf(NetconfError::Transport(t)) => match t {
            TransportError::Auth(_) => ErrorKind::Auth,
            _ => ErrorKind::Connect,
        },
        RustEzError::Netconf(NetconfError::Framing(_)) => ErrorKind::Connect,
        RustEzError::Netconf(NetconfError::Protocol(p)) => match p {
            ProtocolError::SessionClosed
            | ProtocolError::SessionExpired
            | ProtocolError::HelloFailed(_) => ErrorKind::Connect,
            _ => ErrorKind::Internal,
        },
        RustEzError::Netconf(NetconfError::Rpc(r)) => match r {
            RpcError::CommitUnknown => ErrorKind::Commit,
            RpcError::ParseError(_) | RpcError::MessageIdMismatch { .. } => ErrorKind::Internal,
            _ => kind_for_phase(phase),
        },
        RustEzError::Timeout(_) => kind_for_phase(phase),
        RustEzError::Config(_) => kind_for_phase(phase),
        RustEzError::Rpc(_) | RustEzError::Facts(_) => ErrorKind::Rpc,
        RustEzError::SshConfig(_) => ErrorKind::Usage,
        RustEzError::NotConnected | RustEzError::XmlParse(_) => ErrorKind::Internal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustnetconf::error::{NetconfError, RpcError, TransportError};

    #[test]
    fn exit_codes_are_distinct_and_match_spec() {
        assert_eq!(ErrorKind::Usage.exit_code(), 1);
        assert_eq!(ErrorKind::Connect.exit_code(), 2);
        assert_eq!(ErrorKind::Auth.exit_code(), 3);
        assert_eq!(ErrorKind::Rpc.exit_code(), 4);
        assert_eq!(ErrorKind::Load.exit_code(), 5);
        assert_eq!(ErrorKind::Commit.exit_code(), 6);
        assert_eq!(ErrorKind::Rollback.exit_code(), 7);
        assert_eq!(ErrorKind::Internal.exit_code(), 8);
    }

    #[test]
    fn auth_transport_error_classifies_as_auth() {
        let err = RustEzError::Netconf(NetconfError::Transport(TransportError::Auth(
            "bad creds".into(),
        )));
        assert_eq!(classify(&err, Phase::Connect), ErrorKind::Auth);
    }

    #[test]
    fn connect_transport_error_classifies_as_connect() {
        let err = RustEzError::Netconf(NetconfError::Transport(TransportError::Connect(
            "refused".into(),
        )));
        assert_eq!(classify(&err, Phase::Connect), ErrorKind::Connect);
    }

    #[test]
    fn host_key_mismatch_classifies_as_connect() {
        let err = RustEzError::Netconf(NetconfError::Transport(TransportError::HostKeyMismatch {
            host: "h".into(),
            expected: "a".into(),
            actual: "b".into(),
        }));
        assert_eq!(classify(&err, Phase::Facts), ErrorKind::Connect);
    }

    #[test]
    fn server_error_uses_phase_during_load() {
        let err = RustEzError::Netconf(NetconfError::Rpc(RpcError::ServerError {
            error_type: None,
            tag: rustnetconf::types::ErrorTag::OperationFailed,
            severity: None,
            app_tag: None,
            path: None,
            message: "config rejected".into(),
            info: None,
        }));
        assert_eq!(classify(&err, Phase::Load), ErrorKind::Load);
        assert_eq!(classify(&err, Phase::Commit), ErrorKind::Commit);
        assert_eq!(classify(&err, Phase::Rollback), ErrorKind::Rollback);
    }

    #[test]
    fn config_error_uses_phase() {
        let err = RustEzError::Config("nope".into());
        assert_eq!(classify(&err, Phase::Load), ErrorKind::Load);
        assert_eq!(classify(&err, Phase::Commit), ErrorKind::Commit);
    }

    #[test]
    fn not_connected_is_internal() {
        assert_eq!(
            classify(&RustEzError::NotConnected, Phase::Rpc),
            ErrorKind::Internal
        );
    }
}
```

> Note: `ErrorTag::OperationFailed` is the variant name used in the test — confirm the exact `rustnetconf::types::ErrorTag` variant when implementing (run `cargo doc` or grep the dep); pick any real variant, the test only needs a constructible value.

- [ ] **Step 2: Add the `rustnetconf` dependency to the CLI crate**

The classifier imports `rustnetconf::error::*`. Add it to `rustez-cli/Cargo.toml` `[dependencies]`:

```toml
rustnetconf = "0.12"
```

Run: `cargo check -p rustez-cli`
Expected: FAIL — `error` module not yet wired into a crate root (no `mod error;`). That's fine; next step wires modules. If it fails only because `main.rs` doesn't declare `mod error;`, proceed.

- [ ] **Step 3: Declare the module so tests can run**

Add to the **top** of `rustez-cli/src/main.rs` (above `fn main`):

```rust
mod error;
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p rustez-cli error::`
Expected: all `error::tests::*` PASS. If `ErrorTag::OperationFailed` doesn't exist, swap to a real variant and re-run.

- [ ] **Step 5: Commit**

```bash
git add rustez-cli/Cargo.toml rustez-cli/src/error.rs rustez-cli/src/main.rs Cargo.lock
git commit -m "feat(cli): add error taxonomy, phases, and RustEzError classifier"
```

---

## Task 3: clap grammar (`cli.rs`)

**Files:**
- Create: `rustez-cli/src/cli.rs`
- Modify: `rustez-cli/src/main.rs` (add `mod cli;`)

- [ ] **Step 1: Write `cli.rs`**

```rust
//! Command-line grammar for rustez-cli (clap derive).

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Top-level CLI entrypoint.
#[derive(Parser, Debug)]
#[command(name = "rustez", version, about = "Junos device automation from the terminal")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level commands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Gather and print device facts.
    Facts(FactsArgs),
    /// Run an operational CLI command on the device.
    Rpc(RpcArgs),
    /// Configuration operations.
    Config(ConfigArgs),
}

/// Connection options shared by every command.
#[derive(Args, Debug)]
pub struct ConnOpts {
    /// Device hostname or IP.
    pub host: String,
    /// Login username.
    #[arg(short, long)]
    pub user: String,
    /// Login password (insecure: visible in process list — prefer $RUSTEZ_PASSWORD).
    #[arg(short, long)]
    pub password: Option<String>,
    /// NETCONF port (library default if unset).
    #[arg(long)]
    pub port: Option<u16>,
    /// Path to an SSH private key for key-based auth.
    #[arg(long)]
    pub key_file: Option<String>,
    /// Expected SSH host-key fingerprint (e.g. SHA256:...).
    #[arg(long, group = "hostkey")]
    pub host_key_fingerprint: Option<String>,
    /// Path to a known_hosts file for host-key verification.
    #[arg(long, group = "hostkey")]
    pub known_hosts: Option<String>,
    /// Accept any host key (LAB ONLY — disables verification).
    #[arg(long, group = "hostkey")]
    pub accept_any_host_key: bool,
    /// Per-RPC timeout in seconds.
    #[arg(long)]
    pub timeout: Option<u64>,
    /// Emit machine-readable JSON output.
    #[arg(long)]
    pub json: bool,
}

/// Output format for `rpc`.
#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum RpcFormat {
    Text,
    Xml,
}

impl RpcFormat {
    /// The string Junos expects in the `<command format="...">` attribute.
    pub fn as_junos(self) -> &'static str {
        match self {
            RpcFormat::Text => "text",
            RpcFormat::Xml => "xml",
        }
    }
}

/// Config load payload format.
#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum ConfigFormat {
    Set,
    Text,
    Xml,
}

#[derive(Args, Debug)]
pub struct FactsArgs {
    #[command(flatten)]
    pub conn: ConnOpts,
}

#[derive(Args, Debug)]
pub struct RpcArgs {
    #[command(flatten)]
    pub conn: ConnOpts,
    /// Operational CLI command, e.g. "show interfaces terse".
    pub rpc_command: String,
    /// Output format requested from the device.
    #[arg(long, value_enum, default_value_t = RpcFormat::Text)]
    pub format: RpcFormat,
}

#[derive(Args, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// Load a config file and commit it.
    Apply(ConfigLoadArgs),
    /// Load a config file and show the candidate diff (no commit).
    Diff(ConfigLoadArgs),
    /// Load a config file and validate it (no commit).
    CommitCheck(ConfigLoadArgs),
    /// Load a config file and commit, with optional confirm timer/comment.
    Commit(ConfigCommitArgs),
    /// Issue a bare confirming commit (confirms a prior `commit --confirm-minutes`).
    Confirm(ConfigConfirmArgs),
    /// Roll back to a previous configuration and commit.
    Rollback(ConfigRollbackArgs),
}

/// Args for commands that load a file: apply, diff, commit-check.
#[derive(Args, Debug)]
pub struct ConfigLoadArgs {
    #[command(flatten)]
    pub conn: ConnOpts,
    /// Path to the configuration file to load.
    #[arg(short, long)]
    pub file: String,
    /// Format of the configuration file.
    #[arg(long, value_enum, default_value_t = ConfigFormat::Set)]
    pub format: ConfigFormat,
}

#[derive(Args, Debug)]
pub struct ConfigCommitArgs {
    #[command(flatten)]
    pub conn: ConnOpts,
    /// Path to the configuration file to load.
    #[arg(short, long)]
    pub file: String,
    /// Format of the configuration file.
    #[arg(long, value_enum, default_value_t = ConfigFormat::Set)]
    pub format: ConfigFormat,
    /// Arm a confirmed commit that auto-rolls-back after N minutes unless confirmed.
    #[arg(long)]
    pub confirm_minutes: Option<u32>,
    /// Commit log comment.
    #[arg(long)]
    pub comment: Option<String>,
}

#[derive(Args, Debug)]
pub struct ConfigConfirmArgs {
    #[command(flatten)]
    pub conn: ConnOpts,
}

#[derive(Args, Debug)]
pub struct ConfigRollbackArgs {
    #[command(flatten)]
    pub conn: ConnOpts,
    /// Rollback id (0 = previous commit).
    #[arg(long, default_value_t = 0)]
    pub id: u32,
}

impl Cli {
    /// The shared connection options for whichever command was selected.
    pub fn conn(&self) -> &ConnOpts {
        match &self.command {
            Command::Facts(a) => &a.conn,
            Command::Rpc(a) => &a.conn,
            Command::Config(c) => match &c.command {
                ConfigCommand::Apply(a)
                | ConfigCommand::Diff(a)
                | ConfigCommand::CommitCheck(a) => &a.conn,
                ConfigCommand::Commit(a) => &a.conn,
                ConfigCommand::Confirm(a) => &a.conn,
                ConfigCommand::Rollback(a) => &a.conn,
            },
        }
    }

    /// Stable command name used in the JSON envelope `command` field.
    pub fn command_name(&self) -> &'static str {
        match &self.command {
            Command::Facts(_) => "facts",
            Command::Rpc(_) => "rpc",
            Command::Config(c) => match &c.command {
                ConfigCommand::Apply(_) => "config apply",
                ConfigCommand::Diff(_) => "config diff",
                ConfigCommand::CommitCheck(_) => "config commit-check",
                ConfigCommand::Commit(_) => "config commit",
                ConfigCommand::Confirm(_) => "config confirm",
                ConfigCommand::Rollback(_) => "config rollback",
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_facts_with_required_args() {
        let cli = Cli::try_parse_from(["rustez", "facts", "10.0.0.1", "-u", "admin"]).unwrap();
        assert_eq!(cli.command_name(), "facts");
        assert_eq!(cli.conn().host, "10.0.0.1");
        assert_eq!(cli.conn().user, "admin");
        assert!(!cli.conn().json);
    }

    #[test]
    fn facts_requires_user() {
        let res = Cli::try_parse_from(["rustez", "facts", "10.0.0.1"]);
        assert!(res.is_err());
    }

    #[test]
    fn parses_rpc_command_and_format() {
        let cli = Cli::try_parse_from([
            "rustez", "rpc", "10.0.0.1", "show interfaces terse", "-u", "admin", "--format", "xml",
        ])
        .unwrap();
        match &cli.command {
            Command::Rpc(a) => {
                assert_eq!(a.rpc_command, "show interfaces terse");
                assert_eq!(a.format.as_junos(), "xml");
            }
            _ => panic!("expected rpc"),
        }
    }

    #[test]
    fn host_key_flags_are_mutually_exclusive() {
        let res = Cli::try_parse_from([
            "rustez",
            "facts",
            "10.0.0.1",
            "-u",
            "admin",
            "--accept-any-host-key",
            "--known-hosts",
            "/tmp/kh",
        ]);
        assert!(res.is_err(), "two host-key flags should conflict");
    }

    #[test]
    fn parses_config_commit_with_confirm() {
        let cli = Cli::try_parse_from([
            "rustez", "config", "commit", "10.0.0.1", "-u", "admin", "-f", "c.set",
            "--confirm-minutes", "5", "--json",
        ])
        .unwrap();
        assert_eq!(cli.command_name(), "config commit");
        assert!(cli.conn().json);
        match &cli.command {
            Command::Config(c) => match &c.command {
                ConfigCommand::Commit(a) => assert_eq!(a.confirm_minutes, Some(5)),
                _ => panic!("expected commit"),
            },
            _ => panic!("expected config"),
        }
    }

    #[test]
    fn rollback_id_defaults_to_zero() {
        let cli =
            Cli::try_parse_from(["rustez", "config", "rollback", "10.0.0.1", "-u", "admin"]).unwrap();
        match &cli.command {
            Command::Config(c) => match &c.command {
                ConfigCommand::Rollback(a) => assert_eq!(a.id, 0),
                _ => panic!("expected rollback"),
            },
            _ => panic!("expected config"),
        }
    }
}
```

- [ ] **Step 2: Wire the module**

Add to `rustez-cli/src/main.rs` (below `mod error;`):

```rust
mod cli;
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p rustez-cli cli::`
Expected: all `cli::tests::*` PASS. If the mutually-exclusive test fails, confirm all three host-key args share `group = "hostkey"` (clap rejects >1 arg in a group by default).

- [ ] **Step 4: Commit**

```bash
git add rustez-cli/src/cli.rs rustez-cli/src/main.rs
git commit -m "feat(cli): define clap command grammar and arg structs"
```

---

## Task 4: Output envelope and rendering (`output.rs`)

**Files:**
- Create: `rustez-cli/src/output.rs`
- Modify: `rustez-cli/src/main.rs` (add `mod output;`)

- [ ] **Step 1: Write `output.rs`**

```rust
//! JSON envelope, command payloads, and human-text rendering.

use serde::Serialize;

use crate::error::CliError;

/// The stable top-level shape emitted in `--json` mode for every command.
#[derive(Serialize)]
pub struct Envelope<'a> {
    pub ok: bool,
    pub command: &'a str,
    pub host: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<&'a CommandData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

/// The `error` member of a failure envelope.
#[derive(Serialize)]
pub struct ErrorBody {
    pub kind: String,
    pub message: String,
}

impl<'a> Envelope<'a> {
    /// Build a success envelope wrapping a command payload.
    pub fn success(command: &'a str, host: &'a str, data: &'a CommandData) -> Self {
        Envelope {
            ok: true,
            command,
            host,
            data: Some(data),
            error: None,
        }
    }

    /// Build a failure envelope from a CLI error.
    pub fn failure(command: &'a str, host: &'a str, err: &CliError) -> Self {
        Envelope {
            ok: false,
            command,
            host,
            data: None,
            error: Some(ErrorBody {
                kind: err.kind.as_str().to_string(),
                message: err.message.clone(),
            }),
        }
    }
}

/// Per-command result payload. Serializes untagged so `data` is the bare object.
#[derive(Serialize)]
#[serde(untagged)]
pub enum CommandData {
    Facts(rustez::Facts),
    Rpc {
        output: String,
        format: String,
    },
    Commit {
        loaded: bool,
        committed: bool,
        confirm_minutes: Option<u32>,
        warnings: Vec<String>,
    },
    CommitCheck {
        loaded: bool,
        check_passed: bool,
        warnings: Vec<String>,
    },
    Diff {
        diff: Option<String>,
    },
    Confirm {
        committed: bool,
    },
    Rollback {
        rolled_back: bool,
        id: u32,
    },
}

impl CommandData {
    /// Warnings to surface on stderr in text mode (empty for variants w/o warnings).
    pub fn warnings(&self) -> &[String] {
        match self {
            CommandData::Commit { warnings, .. } | CommandData::CommitCheck { warnings, .. } => {
                warnings
            }
            _ => &[],
        }
    }

    /// Render the payload for human (non-JSON) stdout output.
    pub fn render_text(&self) -> String {
        match self {
            CommandData::Facts(f) => format!(
                "hostname:      {}\nmodel:         {}\nversion:       {}\nserial:        {}\npersonality:   {}\ncluster:       {}",
                f.hostname, f.model, f.version, f.serial_number, f.personality, f.is_cluster
            ),
            CommandData::Rpc { output, .. } => output.clone(),
            CommandData::Commit {
                confirm_minutes, ..
            } => match confirm_minutes {
                Some(m) => format!("committed (confirmed, auto-rollback in {m} min)"),
                None => "committed".to_string(),
            },
            CommandData::CommitCheck { .. } => "commit check passed".to_string(),
            CommandData::Diff { diff } => match diff {
                Some(d) => d.clone(),
                None => "(no changes)".to_string(),
            },
            CommandData::Confirm { .. } => "commit confirmed".to_string(),
            CommandData::Rollback { id, .. } => format!("rolled back to {id} and committed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_facts() -> rustez::Facts {
        rustez::Facts {
            hostname: "vsrx-1".into(),
            model: "vSRX".into(),
            version: "24.4R1".into(),
            serial_number: "ABC123".into(),
            personality: rustez::Personality::Vsrx,
            route_engines: vec![],
            master_re: None,
            domain: None,
            fqdn: None,
            is_cluster: false,
        }
    }

    #[test]
    fn success_envelope_facts_has_expected_shape() {
        let data = CommandData::Facts(sample_facts());
        let env = Envelope::success("facts", "10.0.0.1", &data);
        let v: serde_json::Value = serde_json::to_value(&env).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["command"], "facts");
        assert_eq!(v["host"], "10.0.0.1");
        assert_eq!(v["data"]["hostname"], "vsrx-1");
        assert_eq!(v["data"]["personality"], "vsrx");
        assert!(v.get("error").is_none());
    }

    #[test]
    fn failure_envelope_has_kind_and_message() {
        let err = CliError::new(crate::error::ErrorKind::Auth, "authentication failed");
        let env = Envelope::failure("facts", "10.0.0.1", &err);
        let v: serde_json::Value = serde_json::to_value(&env).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"]["kind"], "auth");
        assert_eq!(v["error"]["message"], "authentication failed");
        assert!(v.get("data").is_none());
    }

    #[test]
    fn rpc_data_serializes_bare() {
        let data = CommandData::Rpc {
            output: "up up".into(),
            format: "text".into(),
        };
        let v = serde_json::to_value(&data).unwrap();
        assert_eq!(v["output"], "up up");
        assert_eq!(v["format"], "text");
    }

    #[test]
    fn commit_text_mentions_confirm_window() {
        let data = CommandData::Commit {
            loaded: true,
            committed: true,
            confirm_minutes: Some(5),
            warnings: vec![],
        };
        assert!(data.render_text().contains("5 min"));
    }

    #[test]
    fn commit_warnings_exposed() {
        let data = CommandData::Commit {
            loaded: true,
            committed: true,
            confirm_minutes: None,
            warnings: vec!["mgd: statement deprecated".into()],
        };
        assert_eq!(data.warnings().len(), 1);
    }
}
```

- [ ] **Step 2: Wire the module**

Add to `rustez-cli/src/main.rs`:

```rust
mod output;
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p rustez-cli output::`
Expected: all `output::tests::*` PASS. If `Facts { .. }` construction fails to compile, check field names against `rustez/src/facts/mod.rs` (`hostname, model, version, serial_number, personality, route_engines, master_re, domain, fqdn, is_cluster`).

- [ ] **Step 4: Commit**

```bash
git add rustez-cli/src/output.rs rustez-cli/src/main.rs
git commit -m "feat(cli): add JSON envelope, command payloads, and text rendering"
```

---

## Task 5: Connection setup and credential resolution (`connect.rs`)

**Files:**
- Create: `rustez-cli/src/connect.rs`
- Modify: `rustez-cli/src/main.rs` (add `mod connect;`)

- [ ] **Step 1: Write `connect.rs`**

```rust
//! Device connection setup: credential resolution and host-key policy mapping.

use std::io::IsTerminal;
use std::time::Duration;

use rustez::{Device, HostKeyVerification};

use crate::cli::ConnOpts;
use crate::error::{CliError, ErrorKind, Phase};

/// How the password should be obtained, decided purely from inputs (testable).
#[derive(Debug, PartialEq, Eq)]
pub enum PasswordPlan {
    /// Use this password value directly.
    Use(String),
    /// Prompt interactively (no echo).
    Prompt,
    /// No password needed — key-based auth.
    KeyOnly,
}

/// Decide how to obtain the password. Precedence: flag > env > key > prompt.
///
/// Returns a `usage` error when no source is available and stdin is not a TTY.
pub fn plan_password(
    flag: Option<&str>,
    env: Option<&str>,
    has_key: bool,
    is_tty: bool,
) -> Result<PasswordPlan, CliError> {
    if let Some(p) = flag {
        return Ok(PasswordPlan::Use(p.to_string()));
    }
    if let Some(e) = env {
        return Ok(PasswordPlan::Use(e.to_string()));
    }
    if has_key {
        return Ok(PasswordPlan::KeyOnly);
    }
    if is_tty {
        return Ok(PasswordPlan::Prompt);
    }
    Err(CliError::new(
        ErrorKind::Usage,
        "no password provided and stdin is not a TTY; set $RUSTEZ_PASSWORD or use --key-file",
    ))
}

/// Map host-key CLI flags to a verification policy. `None` => library default (RejectAll).
pub fn host_key_policy(conn: &ConnOpts) -> Option<HostKeyVerification> {
    if let Some(fp) = &conn.host_key_fingerprint {
        return Some(HostKeyVerification::Fingerprint(fp.clone()));
    }
    if let Some(path) = &conn.known_hosts {
        return Some(HostKeyVerification::KnownHosts(path.into()));
    }
    if conn.accept_any_host_key {
        return Some(HostKeyVerification::AcceptAll);
    }
    None
}

/// Build and open a `Device` from connection options.
///
/// `gather_facts` controls whether facts are auto-gathered on open (true for
/// the `facts` command, false for `rpc`/`config` to save three RPCs).
pub async fn build_device(conn: &ConnOpts, gather_facts: bool) -> Result<Device, CliError> {
    if conn.password.is_some() {
        eprintln!(
            "warning: --password is visible in the process list; prefer $RUSTEZ_PASSWORD or --key-file"
        );
    }

    let env_pw = std::env::var("RUSTEZ_PASSWORD").ok();
    let has_key = conn.key_file.is_some();
    let is_tty = std::io::stdin().is_terminal();
    let plan = plan_password(conn.password.as_deref(), env_pw.as_deref(), has_key, is_tty)?;

    let password = match plan {
        PasswordPlan::Use(p) => Some(p),
        PasswordPlan::KeyOnly => None,
        PasswordPlan::Prompt => {
            let prompt = format!("Password for {}@{}: ", conn.user, conn.host);
            let pw = rpassword::prompt_password(prompt)
                .map_err(|e| CliError::new(ErrorKind::Usage, format!("failed to read password: {e}")))?;
            Some(pw)
        }
    };

    let mut builder = Device::connect(&conn.host).username(&conn.user);
    if let Some(pw) = &password {
        builder = builder.password(pw);
    }
    if let Some(kf) = &conn.key_file {
        builder = builder.key_file(kf);
    }
    if let Some(port) = conn.port {
        builder = builder.port(port);
    }
    if let Some(secs) = conn.timeout {
        builder = builder.rpc_timeout(Duration::from_secs(secs));
    }
    if let Some(policy) = host_key_policy(conn) {
        builder = builder.host_key_verification(policy);
    }
    if !gather_facts {
        builder = builder.no_facts();
    }

    builder
        .open()
        .await
        .map_err(|e| CliError::from_rustez(&e, Phase::Connect))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_password_takes_precedence() {
        let plan = plan_password(Some("flagpw"), Some("envpw"), false, false).unwrap();
        assert_eq!(plan, PasswordPlan::Use("flagpw".into()));
    }

    #[test]
    fn env_password_used_when_no_flag() {
        let plan = plan_password(None, Some("envpw"), false, false).unwrap();
        assert_eq!(plan, PasswordPlan::Use("envpw".into()));
    }

    #[test]
    fn key_only_when_no_password_source() {
        let plan = plan_password(None, None, true, false).unwrap();
        assert_eq!(plan, PasswordPlan::KeyOnly);
    }

    #[test]
    fn prompt_when_tty_and_no_other_source() {
        let plan = plan_password(None, None, false, true).unwrap();
        assert_eq!(plan, PasswordPlan::Prompt);
    }

    #[test]
    fn usage_error_when_no_source_and_not_tty() {
        let err = plan_password(None, None, false, false).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Usage);
    }

    #[test]
    fn fingerprint_flag_maps_to_policy() {
        let conn = test_conn(|c| c.host_key_fingerprint = Some("SHA256:x".into()));
        assert!(matches!(
            host_key_policy(&conn),
            Some(HostKeyVerification::Fingerprint(_))
        ));
    }

    #[test]
    fn no_host_key_flag_returns_none() {
        let conn = test_conn(|_| {});
        assert!(host_key_policy(&conn).is_none());
    }

    /// Build a default ConnOpts and let the closure tweak it.
    fn test_conn(tweak: impl FnOnce(&mut ConnOpts)) -> ConnOpts {
        let mut conn = ConnOpts {
            host: "h".into(),
            user: "u".into(),
            password: None,
            port: None,
            key_file: None,
            host_key_fingerprint: None,
            known_hosts: None,
            accept_any_host_key: false,
            timeout: None,
            json: false,
        };
        tweak(&mut conn);
        conn
    }
}
```

- [ ] **Step 2: Wire the module**

Add to `rustez-cli/src/main.rs`:

```rust
mod connect;
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p rustez-cli connect::`
Expected: all `connect::tests::*` PASS.

- [ ] **Step 4: Commit**

```bash
git add rustez-cli/src/connect.rs rustez-cli/src/main.rs
git commit -m "feat(cli): add credential resolution and device connection setup"
```

---

## Task 6: Command handlers (`commands/`)

**Files:**
- Create: `rustez-cli/src/commands/mod.rs`
- Create: `rustez-cli/src/commands/facts.rs`
- Create: `rustez-cli/src/commands/rpc.rs`
- Create: `rustez-cli/src/commands/config.rs`
- Modify: `rustez-cli/src/main.rs` (add `mod commands;`)

> These handlers do live device I/O, so they have no unit tests (covered by the integration test in Task 8 and compile-checked here). Keep them small and delegate to the library.

- [ ] **Step 1: Write `commands/mod.rs`**

```rust
//! Command handlers. Each returns a `CommandData` payload or a `CliError`.

pub mod config;
pub mod facts;
pub mod rpc;
```

- [ ] **Step 2: Write `commands/facts.rs`**

```rust
//! `rustez facts` handler.

use crate::cli::FactsArgs;
use crate::connect::build_device;
use crate::error::{CliError, Phase};
use crate::output::CommandData;

/// Connect, gather facts, return them.
pub async fn run(args: &FactsArgs) -> Result<CommandData, CliError> {
    let mut dev = build_device(&args.conn, true).await?;
    let facts = dev
        .facts()
        .await
        .map_err(|e| CliError::from_rustez(&e, Phase::Facts))?
        .clone();
    let _ = dev.close().await;
    Ok(CommandData::Facts(facts))
}
```

- [ ] **Step 3: Write `commands/rpc.rs`**

```rust
//! `rustez rpc` handler.

use crate::cli::RpcArgs;
use crate::connect::build_device;
use crate::error::{CliError, Phase};
use crate::output::CommandData;

/// Connect (no facts) and run an operational CLI command.
pub async fn run(args: &RpcArgs) -> Result<CommandData, CliError> {
    let mut dev = build_device(&args.conn, false).await?;
    let format = args.format.as_junos();
    let output = {
        let mut executor = dev.rpc().map_err(|e| CliError::from_rustez(&e, Phase::Rpc))?;
        executor
            .cli(&args.rpc_command, format)
            .await
            .map_err(|e| CliError::from_rustez(&e, Phase::Rpc))?
    };
    let _ = dev.close().await;
    Ok(CommandData::Rpc {
        output,
        format: format.to_string(),
    })
}
```

- [ ] **Step 4: Write `commands/config.rs`**

```rust
//! `rustez config ...` handlers.

use rustez::{ConfigPayload, Device};

use crate::cli::{
    ConfigCommitArgs, ConfigConfirmArgs, ConfigFormat, ConfigLoadArgs, ConfigRollbackArgs,
};
use crate::connect::build_device;
use crate::error::{CliError, ErrorKind, Phase};
use crate::output::CommandData;

/// Read a config file into a `ConfigPayload` for the requested format.
fn read_payload(file: &str, format: ConfigFormat) -> Result<ConfigPayload, CliError> {
    let content = std::fs::read_to_string(file)
        .map_err(|e| CliError::new(ErrorKind::Usage, format!("cannot read {file}: {e}")))?;
    Ok(match format {
        ConfigFormat::Set => ConfigPayload::Set(content),
        ConfigFormat::Text => ConfigPayload::Text(content),
        ConfigFormat::Xml => ConfigPayload::Xml(content),
    })
}

/// Lock, load (capturing warnings). On error, the caller closes the device,
/// which releases the candidate lock — so no explicit unlock on the error path.
async fn lock_and_load(
    dev: &mut Device,
    payload: ConfigPayload,
) -> Result<Vec<String>, CliError> {
    let mut cfg = dev
        .config()
        .map_err(|e| CliError::from_rustez(&e, Phase::Load))?;
    cfg.lock()
        .await
        .map_err(|e| CliError::from_rustez(&e, Phase::Load))?;
    let (_resp, warnings) = cfg
        .load_with_warnings(payload)
        .await
        .map_err(|e| CliError::from_rustez(&e, Phase::Load))?;
    Ok(warnings.iter().map(|w| w.message.clone()).collect())
}

/// `config apply` — load and commit (the simple convenience verb).
pub async fn apply(args: &ConfigLoadArgs) -> Result<CommandData, CliError> {
    let payload = read_payload(&args.file, args.format)?;
    let mut dev = build_device(&args.conn, false).await?;
    let result = apply_inner(&mut dev, payload, None, None).await;
    let _ = dev.close().await;
    result
}

/// `config commit` — load and commit with optional confirm timer/comment.
pub async fn commit(args: &ConfigCommitArgs) -> Result<CommandData, CliError> {
    let payload = read_payload(&args.file, args.format)?;
    let mut dev = build_device(&args.conn, false).await?;
    let result = apply_inner(
        &mut dev,
        payload,
        args.confirm_minutes,
        args.comment.as_deref(),
    )
    .await;
    let _ = dev.close().await;
    result
}

/// Shared load + commit + unlock used by `apply` and `commit`.
async fn apply_inner(
    dev: &mut Device,
    payload: ConfigPayload,
    confirm_minutes: Option<u32>,
    comment: Option<&str>,
) -> Result<CommandData, CliError> {
    let warnings = lock_and_load(dev, payload).await?;
    let mut cfg = dev
        .config()
        .map_err(|e| CliError::from_rustez(&e, Phase::Commit))?;
    let commit_result = if let Some(mins) = confirm_minutes {
        cfg.commit_confirmed(mins * 60).await
    } else if let Some(c) = comment {
        cfg.commit_with_comment(c).await
    } else {
        cfg.commit().await
    };
    commit_result.map_err(|e| CliError::from_rustez(&e, Phase::Commit))?;
    cfg.unlock()
        .await
        .map_err(|e| CliError::from_rustez(&e, Phase::Commit))?;
    Ok(CommandData::Commit {
        loaded: true,
        committed: true,
        confirm_minutes,
        warnings,
    })
}

/// `config commit-check` — load and validate without committing.
pub async fn commit_check(args: &ConfigLoadArgs) -> Result<CommandData, CliError> {
    let payload = read_payload(&args.file, args.format)?;
    let mut dev = build_device(&args.conn, false).await?;
    let result = commit_check_inner(&mut dev, payload).await;
    let _ = dev.close().await;
    result
}

async fn commit_check_inner(
    dev: &mut Device,
    payload: ConfigPayload,
) -> Result<CommandData, CliError> {
    let warnings = lock_and_load(dev, payload).await?;
    let mut cfg = dev
        .config()
        .map_err(|e| CliError::from_rustez(&e, Phase::Commit))?;
    cfg.commit_check()
        .await
        .map_err(|e| CliError::from_rustez(&e, Phase::Commit))?;
    cfg.unlock()
        .await
        .map_err(|e| CliError::from_rustez(&e, Phase::Commit))?;
    Ok(CommandData::CommitCheck {
        loaded: true,
        check_passed: true,
        warnings,
    })
}

/// `config diff` — load a file and return the candidate diff (no commit).
pub async fn diff(args: &ConfigLoadArgs) -> Result<CommandData, CliError> {
    let payload = read_payload(&args.file, args.format)?;
    let mut dev = build_device(&args.conn, false).await?;
    let result = diff_inner(&mut dev, payload).await;
    let _ = dev.close().await;
    result
}

async fn diff_inner(dev: &mut Device, payload: ConfigPayload) -> Result<CommandData, CliError> {
    let _warnings = lock_and_load(dev, payload).await?;
    let mut cfg = dev
        .config()
        .map_err(|e| CliError::from_rustez(&e, Phase::Load))?;
    let diff = cfg
        .diff()
        .await
        .map_err(|e| CliError::from_rustez(&e, Phase::Load))?;
    cfg.unlock()
        .await
        .map_err(|e| CliError::from_rustez(&e, Phase::Load))?;
    Ok(CommandData::Diff { diff })
}

/// `config confirm` — bare confirming commit for a prior confirmed commit.
pub async fn confirm(args: &ConfigConfirmArgs) -> Result<CommandData, CliError> {
    let mut dev = build_device(&args.conn, false).await?;
    let result = confirm_inner(&mut dev).await;
    let _ = dev.close().await;
    result
}

async fn confirm_inner(dev: &mut Device) -> Result<CommandData, CliError> {
    let mut cfg = dev
        .config()
        .map_err(|e| CliError::from_rustez(&e, Phase::Commit))?;
    cfg.commit()
        .await
        .map_err(|e| CliError::from_rustez(&e, Phase::Commit))?;
    Ok(CommandData::Confirm { committed: true })
}

/// `config rollback` — roll back to an id and commit.
pub async fn rollback(args: &ConfigRollbackArgs) -> Result<CommandData, CliError> {
    let mut dev = build_device(&args.conn, false).await?;
    let result = rollback_inner(&mut dev, args.id).await;
    let _ = dev.close().await;
    result
}

async fn rollback_inner(dev: &mut Device, id: u32) -> Result<CommandData, CliError> {
    let mut cfg = dev
        .config()
        .map_err(|e| CliError::from_rustez(&e, Phase::Rollback))?;
    cfg.lock()
        .await
        .map_err(|e| CliError::from_rustez(&e, Phase::Rollback))?;
    cfg.rollback(id)
        .await
        .map_err(|e| CliError::from_rustez(&e, Phase::Rollback))?;
    cfg.commit()
        .await
        .map_err(|e| CliError::from_rustez(&e, Phase::Rollback))?;
    cfg.unlock()
        .await
        .map_err(|e| CliError::from_rustez(&e, Phase::Rollback))?;
    Ok(CommandData::Rollback {
        rolled_back: true,
        id,
    })
}
```

- [ ] **Step 5: Wire the module**

Add to `rustez-cli/src/main.rs`:

```rust
mod commands;
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo check -p rustez-cli`
Expected: compiles. If borrow-checker complains in `rpc.rs` about `executor` outliving `dev.close()`, confirm the executor is scoped in its own `{ }` block (as written) so the borrow ends before `close()`.

- [ ] **Step 7: Commit**

```bash
git add rustez-cli/src/commands/ rustez-cli/src/main.rs
git commit -m "feat(cli): implement facts, rpc, and config command handlers"
```

---

## Task 7: Wire `main.rs` — parse, dispatch, render, exit codes

**Files:**
- Modify: `rustez-cli/src/main.rs` (replace the placeholder `fn main`)

- [ ] **Step 1: Replace `main.rs` body**

Keep the `mod` declarations at the top (added in earlier tasks) and replace `fn main()` with:

```rust
mod cli;
mod commands;
mod connect;
mod error;
mod output;

use clap::Parser;

use cli::{Cli, Command, ConfigCommand};
use error::CliError;
use output::{CommandData, Envelope};

#[tokio::main]
async fn main() {
    // Parse args. clap handles --help/--version (exit 0); other parse failures
    // are usage errors (exit 1).
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            let code = match e.kind() {
                clap::error::ErrorKind::DisplayHelp
                | clap::error::ErrorKind::DisplayVersion
                | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => 0,
                _ => 1,
            };
            std::process::exit(code);
        }
    };

    let command_name = cli.command_name();
    let host = cli.conn().host.clone();
    let json = cli.conn().json;

    let result = dispatch(&cli).await;

    match result {
        Ok(data) => {
            if json {
                let env = Envelope::success(command_name, &host, &data);
                println!("{}", serde_json::to_string_pretty(&env).unwrap());
            } else {
                println!("{}", data.render_text());
                for w in data.warnings() {
                    eprintln!("warning: {w}");
                }
            }
            std::process::exit(0);
        }
        Err(err) => {
            if json {
                let env = Envelope::failure(command_name, &host, &err);
                eprintln!("{}", serde_json::to_string_pretty(&env).unwrap());
            } else {
                eprintln!("error [{}]: {}", err.kind.as_str(), err.message);
            }
            std::process::exit(err.kind.exit_code());
        }
    }
}

/// Route the parsed command to its handler.
async fn dispatch(cli: &Cli) -> Result<CommandData, CliError> {
    match &cli.command {
        Command::Facts(a) => commands::facts::run(a).await,
        Command::Rpc(a) => commands::rpc::run(a).await,
        Command::Config(c) => match &c.command {
            ConfigCommand::Apply(a) => commands::config::apply(a).await,
            ConfigCommand::Diff(a) => commands::config::diff(a).await,
            ConfigCommand::CommitCheck(a) => commands::config::commit_check(a).await,
            ConfigCommand::Commit(a) => commands::config::commit(a).await,
            ConfigCommand::Confirm(a) => commands::config::confirm(a).await,
            ConfigCommand::Rollback(a) => commands::config::rollback(a).await,
        },
    }
}
```

- [ ] **Step 2: Build and lint**

Run: `cargo build -p rustez-cli && cargo clippy -p rustez-cli`
Expected: builds; clippy clean (or only pre-existing workspace warnings).

- [ ] **Step 3: Smoke-test the CLI surface (no device)**

Run: `cargo run -p rustez-cli -- --help`
Expected: top-level help lists `facts`, `rpc`, `config`.

Run: `cargo run -p rustez-cli -- config --help`
Expected: lists `apply`, `diff`, `commit-check`, `commit`, `confirm`, `rollback`.

Run: `cargo run -p rustez-cli -- facts; echo "exit=$?"`
Expected: usage error printed, `exit=1` (missing required `<HOST>`/`-u`).

- [ ] **Step 4: Run all CLI unit tests**

Run: `cargo test -p rustez-cli`
Expected: all tests from Tasks 2–5 PASS.

- [ ] **Step 5: Commit**

```bash
git add rustez-cli/src/main.rs
git commit -m "feat(cli): wire arg dispatch, envelope rendering, and exit codes"
```

---

## Task 8: Integration test against a real vSRX

**Files:**
- Create: `rustez-cli/tests/cli_integration.rs`

- [ ] **Step 1: Write the ignored integration test**

```rust
//! Integration test for rustez-cli against a real vSRX.
//!
//! Ignored by default. Run with:
//! ```sh
//! RUSTEZ_VSRX_HOST=<IP> RUSTEZ_VSRX_USER=<USER> RUSTEZ_VSRX_PASS=<PASS> \
//!     cargo test -p rustez-cli -- --ignored
//! ```

use std::env;
use std::process::Command;

/// IT: `rustez facts --json` returns exit 0 and a parseable success envelope.
#[test]
#[ignore]
fn facts_json_against_vsrx() {
    let host = env::var("RUSTEZ_VSRX_HOST").expect("RUSTEZ_VSRX_HOST not set");
    let user = env::var("RUSTEZ_VSRX_USER").unwrap_or_else(|_| "admin".to_string());
    let pass = env::var("RUSTEZ_VSRX_PASS").expect("RUSTEZ_VSRX_PASS not set");

    let output = Command::new(env!("CARGO_BIN_EXE_rustez-cli"))
        .args([
            "facts",
            &host,
            "-u",
            &user,
            "-p",
            &pass,
            "--accept-any-host-key",
            "--json",
        ])
        .output()
        .expect("failed to run rustez-cli");

    assert!(
        output.status.success(),
        "exit={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout not utf8");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout not valid JSON");
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "facts");
    assert!(
        v["data"]["hostname"].as_str().is_some_and(|h| !h.is_empty()),
        "expected non-empty hostname, got {v}"
    );
}
```

> `CARGO_BIN_EXE_rustez-cli` is set automatically by Cargo for integration tests of a crate with a binary target. `serde_json` is already a dependency, so it's available to the test.

- [ ] **Step 2: Verify it compiles and is skipped by default**

Run: `cargo test -p rustez-cli`
Expected: the new test shows as `ignored`; all other tests PASS.

- [ ] **Step 3: (Optional, if a vSRX is available) run it**

Run:
```sh
RUSTEZ_VSRX_HOST=<IP> RUSTEZ_VSRX_USER=<USER> RUSTEZ_VSRX_PASS=<PASS> \
  cargo test -p rustez-cli -- --ignored
```
Expected: PASS with a real hostname in the JSON envelope.

- [ ] **Step 4: Commit**

```bash
git add rustez-cli/tests/cli_integration.rs
git commit -m "test(cli): add ignored vSRX integration test for facts --json"
```

---

## Task 9: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Full workspace check**

Run: `cargo check && cargo clippy -p rustez-cli && cargo test -p rustez-cli`
Expected: all green; no clippy warnings introduced by the CLI crate.

- [ ] **Step 2: Confirm exit-code contract by hand (no device needed)**

Run: `cargo run -p rustez-cli -- config apply 10.0.0.1 -u admin -f /nonexistent.set; echo "exit=$?"`
Expected: `exit=1` (usage — file unreadable), error printed to stderr.

Run (JSON form): `cargo run -p rustez-cli -- config apply 10.0.0.1 -u admin -f /nonexistent.set --json 2>&1 1>/dev/null`
Expected: a failure envelope with `"ok": false` and `"kind": "usage"`.

- [ ] **Step 3: Confirm README examples still match**

The README's `rustez facts ... -u admin -p secret`, `rustez rpc ... "show interfaces terse" -u admin`, and `rustez config apply ... -f config.set -u admin` should all parse. Verify each with `--help` on the relevant subcommand. No README edit required unless you add a `--json`/host-key usage note (optional follow-up from the spec).

- [ ] **Step 4: Final commit (if any verification fixes were made)**

```bash
git add -A
git commit -m "chore(cli): final verification fixes"
```

---

## Self-Review Notes (plan author)

- **Spec coverage:** facts ✓ (T6), rpc + `--format` ✓ (T6), config apply/diff/commit-check/commit/confirm/rollback ✓ (T6), JSON envelope ✓ (T4), exit-code taxonomy ✓ (T2), credential precedence + warning + prompt ✓ (T5), host-key flags + RejectAll default ✓ (T5), `--format set|text|xml` w/ xml caveat (documented in spec) ✓ (T6), unit tests for parsing/classifier/rendering/credentials ✓ (T2–T5), one ignored vSRX integration test ✓ (T8).
- **Type consistency:** `CommandData` variants, `ErrorKind`/`Phase`, `ConnOpts` fields, and handler signatures are consistent across tasks. `RpcFormat::as_junos()` and `command_name()`/`conn()` accessors defined once in `cli.rs` and reused.
- **Known confirm-at-implementation item:** exact `rustnetconf::types::ErrorTag` variant used only in a test constructor (T2 Step 1 note) — swap to any real variant if `OperationFailed` differs.
