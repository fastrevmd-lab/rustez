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
