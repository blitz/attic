use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{bail, Result, Context};
use clap::{Parser, ValueEnum};
use tokio::join;
use tokio::task::spawn;
use tracing_error::ErrorLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

use attic_server::config;

/// Nix binary cache server.
#[derive(Debug, Parser)]
#[clap(version, author = "Zhaofeng Li <hello@zhaofeng.li>")]
#[clap(propagate_version = true)]
struct Opts {
    /// Path to the config file.
    #[clap(short = 'f', long)]
    config: Option<PathBuf>,

    /// Socket address to listen on.
    ///
    /// This overrides `listen` in the config.
    #[clap(short = 'l', long)]
    listen: Option<SocketAddr>,

    /// Mode to run.
    #[clap(long, default_value = "monolithic")]
    mode: ServerMode,

    /// Whether to enable tokio-console.
    ///
    /// The console server will listen on its default port.
    #[clap(long)]
    tokio_console: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ServerMode {
    /// Run all components.
    Monolithic,

    /// Run the API server.
    ApiServer,

    /// Run the garbage collector periodically.
    GarbageCollector,

    /// Run the database migrations then exit.
    DbMigrations,

    /// Run garbage collection then exit.
    GarbageCollectorOnce,

    /// Check the configuration then exit.
    CheckConfig,
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Opts::parse();

    init_logging(opts.tokio_console);
    dump_version();
    check_legacy_config()?;

    let config =
        config::load_config(opts.config.as_deref())
            .await.context("Failed to load configuration file. Please refer to the documentation: https://celler.x86.lol/")?;

    match opts.mode {
        ServerMode::Monolithic => {
            attic_server::run_migrations(config.clone()).await?;

            let (api_server, _) = join!(
                attic_server::run_api_server(opts.listen, config.clone()),
                attic_server::gc::run_garbage_collection(config.clone()),
            );

            api_server?;
        }
        ServerMode::ApiServer => {
            attic_server::run_api_server(opts.listen, config).await?;
        }
        ServerMode::GarbageCollector => {
            attic_server::gc::run_garbage_collection(config.clone()).await;
        }
        ServerMode::DbMigrations => {
            attic_server::run_migrations(config).await?;
        }
        ServerMode::GarbageCollectorOnce => {
            attic_server::gc::run_garbage_collection_once(config).await?;
        }
        ServerMode::CheckConfig => {
            // config is valid, let's just exit :)
        }
    }

    Ok(())
}

fn init_logging(tokio_console: bool) {
    let env_filter = EnvFilter::from_default_env();
    let fmt_layer = tracing_subscriber::fmt::layer().with_filter(env_filter);

    let error_layer = ErrorLayer::default();

    let console_layer = if tokio_console {
        let (layer, server) = console_subscriber::ConsoleLayer::new();
        spawn(server.serve());
        Some(layer)
    } else {
        None
    };

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(error_layer)
        .with(console_layer)
        .init();

    if tokio_console {
        eprintln!("Note: tokio-console is enabled");
    }
}

fn dump_version() {
    #[cfg(debug_assertions)]
    eprintln!("Celler {} (debug)", env!("CARGO_PKG_VERSION"));

    #[cfg(not(debug_assertions))]
    eprintln!("Celler {} (release)", env!("CARGO_PKG_VERSION"));
}

fn check_legacy_config() -> Result<()> {
    let obsolete_env_vars = [
        "CELLER_SERVER_CONFIG_BASE64",
        "CELLER_SERVER_TOKEN_HS256_SECRET_BASE64",
        "CELLER_SERVER_TOKEN_RS256_SECRET_BASE64",
        "CELLER_SERVER_TOKEN_RS256_PUBKEY_BASE64",
    ];

    for var in obsolete_env_vars {
        if std::env::var_os(var).is_none() {
            continue;
        }

        bail!("Obsolete environment variable {var} is set. Please adapt your configuration.
See the migration guide: https://celler.x86.lol/admin-guide/attic-migration.html

If you want to stay on a attic-compatible version, use this Git tag: v0.0.1");
    }
    Ok(())
}
