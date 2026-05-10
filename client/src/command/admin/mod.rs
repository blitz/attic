pub mod make_token;

use anyhow::Result;
use clap::{Parser, Subcommand};
use enum_as_inner::EnumAsInner;

use crate::cli::Opts;

/// Commands for managing the Celler server.
#[derive(Debug, Parser)]
pub struct Admin {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand, EnumAsInner)]
pub enum Command {
    MakeToken(make_token::MakeToken),
}

pub async fn run(opts: Opts) -> Result<()> {
    let sub = opts.command.as_admin().unwrap();

    #[allow(clippy::match_single_binding)]
    match &sub.command {
        Command::MakeToken(mt) => make_token::run(mt).await,
    }
}
