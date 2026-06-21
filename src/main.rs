mod cli;
mod config;
mod daemon;
mod error;
mod server;
mod ui;
mod watcher;
mod webdav;

use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

use cli::{Cli, Commands};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Start) => {
            daemon::do_start(cli.foreground)?;
            if !cli.foreground {
                return Ok(());
            }
        }
        Some(Commands::Stop) => {
            daemon::do_stop()?;
            return Ok(());
        }
        Some(Commands::Status) => {
            if cli.clean {
                daemon::clean_pid()?;
            } else {
                daemon::do_status()?;
            }
            return Ok(());
        }
        Some(Commands::Restart) => {
            daemon::do_stop().ok();
            daemon::do_start(false)?;
            return Ok(());
        }
        None => {
            // Run in foreground
        }
    }

    let mut cfg = config::Config::load(&cli.config)?;

    if let Some(host) = &cli.host {
        cfg.server.host = host.clone();
    }
    if let Some(port) = cli.port {
        cfg.server.port = port;
    }

    if cli.tls {
        server::ensure_self_signed_cert(&mut cfg)?;
        info!("HTTPS enabled with auto-generated self-signed certificate");
    }

    info!("DriveShare v{}", env!("CARGO_PKG_VERSION"));
    info!("Configuration loaded");
    info!("Binding to {}:{}", cfg.server.host, cfg.server.port);

    server::start(cfg).await
}
