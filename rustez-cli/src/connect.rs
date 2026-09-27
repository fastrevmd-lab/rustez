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

/// Decide how to obtain the password. Precedence: file > env > key > prompt.
///
/// `file` is the password file's already-read, already-validated contents —
/// this function makes no I/O calls, which is what keeps it plain and
/// testable. There is deliberately no CLI-flag source: a flag value is
/// visible to every other process on the host via `ps`, and to anyone with
/// shell history access.
///
/// Returns a `usage` error when no source is available and stdin is not a TTY.
pub fn plan_password(
    file: Option<&str>,
    env: Option<&str>,
    has_key: bool,
    is_tty: bool,
) -> Result<PasswordPlan, CliError> {
    if let Some(p) = file {
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
        "no password provided and stdin is not a TTY; set $RUSTEZ_PASSWORD, \
         --password-file <PATH>, or use --key-file",
    ))
}

/// Read and validate a password file: a regular, non-symlink file at exactly
/// mode 0600, holding valid UTF-8 with at most one trailing line ending
/// stripped.
///
/// The permission and symlink checks match the credential-file convention
/// used elsewhere at Mechub (e.g. rustmistmcp's `validate_credential_file`):
/// a password file left group- or world-readable, or reached through a
/// symlink an attacker can redirect, defeats the point of moving the secret
/// out of the CLI argument in the first place.
fn read_password_file(path: &str) -> Result<String, CliError> {
    let usage = |message: String| CliError::new(ErrorKind::Usage, message);

    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| usage(format!("--password-file {path}: {e}")))?;
    if metadata.file_type().is_symlink() {
        return Err(usage(format!(
            "--password-file {path} must not be a symlink"
        )));
    }
    if !metadata.file_type().is_file() {
        return Err(usage(format!(
            "--password-file {path} must be a regular file"
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode() & 0o777;
        if mode != 0o600 {
            return Err(usage(format!(
                "--password-file {path} must be mode 0600, found {mode:o}"
            )));
        }
    }

    let raw = std::fs::read(path).map_err(|e| usage(format!("--password-file {path}: {e}")))?;
    let mut text = String::from_utf8(raw)
        .map_err(|_| usage(format!("--password-file {path} is not valid UTF-8")))?;
    // Strip at most one trailing line ending: a password file written with an
    // editor ordinarily ends in one, and a password legitimately ending in a
    // real newline or other whitespace byte is indistinguishable from that at
    // this layer, so trimming more would silently change the credential.
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    if text.is_empty() {
        return Err(usage(format!("--password-file {path} is empty")));
    }
    Ok(text)
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
    let file_pw = conn
        .password_file
        .as_deref()
        .map(read_password_file)
        .transpose()?;
    let env_pw = std::env::var("RUSTEZ_PASSWORD").ok();
    let has_key = conn.key_file.is_some();
    let is_tty = std::io::stdin().is_terminal();
    let plan = plan_password(file_pw.as_deref(), env_pw.as_deref(), has_key, is_tty)?;

    let password = match plan {
        PasswordPlan::Use(p) => Some(p),
        PasswordPlan::KeyOnly => None,
        PasswordPlan::Prompt => {
            let prompt = format!("Password for {}@{}: ", conn.user, conn.host);
            let pw = rpassword::prompt_password(prompt).map_err(|e| {
                CliError::new(ErrorKind::Usage, format!("failed to read password: {e}"))
            })?;
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
    fn file_password_takes_precedence() {
        let plan = plan_password(Some("filepw"), Some("envpw"), false, false).unwrap();
        assert_eq!(plan, PasswordPlan::Use("filepw".into()));
    }

    #[test]
    fn env_password_used_when_no_file() {
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
            password_file: None,
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

    #[cfg(unix)]
    fn write_password_file(
        dir: &std::path::Path,
        name: &str,
        contents: &[u8],
        mode: u32,
    ) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path.to_str().unwrap().to_owned()
    }

    #[cfg(unix)]
    #[test]
    fn password_file_strips_one_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_password_file(dir.path(), "pw", b"hunter2\n", 0o600);
        assert_eq!(read_password_file(&path).unwrap(), "hunter2");
    }

    #[cfg(unix)]
    #[test]
    fn password_file_keeps_internal_and_non_newline_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_password_file(dir.path(), "pw", b"trailing space \n", 0o600);
        assert_eq!(read_password_file(&path).unwrap(), "trailing space ");
    }

    #[cfg(unix)]
    #[test]
    fn password_file_at_0644_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_password_file(dir.path(), "pw", b"hunter2\n", 0o644);
        let err = read_password_file(&path).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Usage);
        assert!(err.message.contains("0600"));
    }

    #[cfg(unix)]
    #[test]
    fn empty_password_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_password_file(dir.path(), "pw", b"", 0o600);
        let err = read_password_file(&path).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Usage);
        assert!(err.message.contains("empty"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_password_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let real = write_password_file(dir.path(), "real-pw", b"hunter2\n", 0o600);
        let link = dir.path().join("linked-pw");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let err = read_password_file(link.to_str().unwrap()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Usage);
        assert!(err.message.contains("symlink"));
    }

    #[test]
    fn missing_password_file_is_a_usage_error() {
        let err = read_password_file("/nonexistent/rustez-password-file").unwrap_err();
        assert_eq!(err.kind, ErrorKind::Usage);
    }
}
