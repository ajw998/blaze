use std::path::PathBuf;

use anyhow::Result;
use blaze_runtime::{blaze_dir, default_index_path, default_scan_root};
use clap::Parser;

#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub root: PathBuf,
    // Path to the index
    pub index_path: PathBuf,
    // Unix domain socket path
    pub socket_path: PathBuf,
    pub deamonize: bool,
}

fn default_socket_path() -> PathBuf {
    blaze_dir().join("daemon.sock")
}

#[derive(Debug, Parser)]
#[command(name = "blaze-daemon", about = "Blaze Daemon")]
pub struct Cli {
    /// Path to index file (optional override)
    #[arg(long)]
    pub index_path: Option<PathBuf>,

    /// Path to Unix domain socket (optional override)
    #[arg(long)]
    pub socket_path: Option<PathBuf>,

    /// Run in background (detach from terminal).
    #[arg(long)]
    pub daemonize: bool,
}

impl DaemonConfig {
    pub fn from_args(args: &Cli) -> Result<Self> {
        let root = default_scan_root();
        let index_path = args.index_path.clone().unwrap_or_else(default_index_path);
        let socket_path = args.socket_path.clone().unwrap_or_else(default_socket_path);

        Ok(Self {
            root,
            index_path,
            socket_path,
            deamonize: args.daemonize,
        })
    }

    pub fn from_env() -> Result<Self> {
        let args = Cli::parse();
        Self::from_args(&args)
    }
}
