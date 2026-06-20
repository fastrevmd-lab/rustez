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
