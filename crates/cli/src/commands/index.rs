use std::{fs, process::ExitCode};

use anyhow::Result;
use blaze_engine::{Index, IndexReader};
use blaze_indexer::build_initial_index;
use blaze_runtime::{default_index_path, default_scan_root};
use clap::{Args, Subcommand};
use log::error;

#[derive(Debug, Args)]
pub struct IndexArgs {
    #[command(subcommand)]
    pub action: IndexAction,
}

#[derive(Debug, Subcommand)]
pub enum IndexAction {
    Info,
    Build {
        /// Force rebuild even if index exists and is valid
        #[arg(long, short = 'f')]
        force: bool,
    },
}

pub fn run(args: IndexArgs) -> ExitCode {
    match execute(args) {
        Ok(code) => code,
        Err(e) => {
            error!("[error] {e}");
            eprintln!("[index] {e}");
            ExitCode::from(2)
        }
    }
}

fn execute(args: IndexArgs) -> Result<ExitCode> {
    match args.action {
        IndexAction::Build { force } => build_index(force),
        IndexAction::Info => show_info(),
    }
}

pub fn build_index(force: bool) -> Result<ExitCode> {
    let _ = force;

    let root = default_scan_root();

    let index_location = default_index_path();

    let (_, atime_warning) = build_initial_index(&root, &index_location, true)?;

    if let Some(msg) = atime_warning {
        eprintln!("{msg}");
    }

    Ok(ExitCode::SUCCESS)
}

fn show_info() -> Result<ExitCode> {
    let index_location = default_index_path();

    if !index_location.exists() {
        eprintln!("[index] no index found at {}", index_location.display());
        // Treat absence as a "soft" failure with non-zero exit
        return Ok(ExitCode::from(1));
    }

    let index = Index::open(&index_location)?;

    let root = index.root_path().unwrap_or("<unknown>");

    // Use the IndexReader API for counts.
    let file_count = index.get_file_count();
    let dir_count = index.dir_count();

    let meta = fs::metadata(&index_location)?;
    let size_bytes = meta.len();

    eprintln!("[index] location: {}", index_location.display());
    eprintln!("[index] root:     {}", root);
    eprintln!("[index] files:    {}", file_count);
    eprintln!("[index] dirs:     {}", dir_count);
    eprintln!("[index] size:     {} bytes", size_bytes);

    Ok(ExitCode::SUCCESS)
}
