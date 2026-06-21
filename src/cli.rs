use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "driveshare",
    version = "0.1.0",
    about = "Share folders on your LAN via WebDAV with a web dashboard",
    long_about = "DriveShare - Share local folders as network drives\n\
                   on your LAN with zero configuration.\n\n\
                   Web UI automatically opens in your browser.\n\
                   Other devices access your shares via the displayed URLs.\n\n\
                   Manage as a background service:\n\
                   driveshare start    Start server in background\n\
                   driveshare stop     Stop background server\n\
                   driveshare status   Check server status\n\
                   driveshare restart  Restart server"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[arg(
        short = 'c',
        long = "config",
        help = "Path to configuration file (TOML format)",
        global = true
    )]
    pub config: Option<String>,

    #[arg(
        short = 'H',
        long = "host",
        help = "Host address to bind to (overrides config)",
        global = true
    )]
    pub host: Option<String>,

    #[arg(
        short = 'P',
        long = "port",
        help = "Port to listen on (overrides config)",
        global = true
    )]
    pub port: Option<u16>,

    #[arg(
        short = 't',
        long = "tls",
        help = "Enable HTTPS with auto-generated self-signed certificate",
        global = true
    )]
    pub tls: bool,

    #[arg(
        long = "foreground",
        help = "Run in foreground (used internally by start command)",
        hide = true,
        global = true
    )]
    pub foreground: bool,

    #[arg(
        long = "clean",
        help = "Remove stale PID file (used with status command)",
        global = true
    )]
    pub clean: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    #[command(about = "Start the server in background (daemon mode)")]
    Start,
    #[command(about = "Stop the background server")]
    Stop,
    #[command(about = "Check if server is running")]
    Status,
    #[command(about = "Restart the server")]
    Restart,
}
