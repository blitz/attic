use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::cli::Opts;

/// Commands for managing the Celler server.
#[derive(Debug, Parser)]
pub struct Admin {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    // Nothing here yet.
}

pub async fn run(opts: Opts) -> Result<()> {
    let sub = opts.command.as_admin().unwrap();

    #[allow(clippy::match_single_binding)]
    match &sub.command {
        _ => todo!(),
    }
}
