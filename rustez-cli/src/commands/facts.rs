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
