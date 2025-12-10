use std::{fs, path::Path, process::ExitCode, sync::Arc, thread};

use anyhow::{Error, Result};
use blaze_engine::{Index, IndexBuilder, IndexReader, StagedIndex, write_index_atomic};
use blaze_fs::{FileRecord, IgnoreEngine, ScanContext, TrashConfig, UserExcludes, walk_parallel};
use blaze_runtime::{default_index_path, default_scan_root};
use clap::{Args, Subcommand};
use crossbeam::channel;
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
        // TODO: Move the build procedure out of this function
        IndexAction::Build { force: _force } => {
            // TODO: use `force` when for compatibility checks / cache.
            let root = default_scan_root();

            let scan_context = create_scan_context()?;

            let index_location = default_index_path();

            let (_staged, atime_warning) = build_index_from_scan(&root, scan_context, true)?;

            if let Some(msg) = atime_warning {
                eprintln!("{msg}");
            }

            let _ = write_index_atomic(&index_location, &_staged, 0);

            Ok(ExitCode::SUCCESS)
        }
        IndexAction::Info => show_info(),
    }
}

fn create_scan_context() -> Result<Arc<ScanContext>> {
    // TODO: Receive configurations
    let ignore = IgnoreEngine::default();

    Ok(Arc::new(ScanContext {
        trash: TrashConfig::new(),
        ignore,
        user_excludes: UserExcludes::new(Vec::new()),
    }))
}

/// Build index from filesystem scan with optional filtering and atime checking.
///
/// Returns (StagedIndex, optional atime warning message).
///
/// * `root` - Root directory to scan
/// * `ctx`  - Shared scan context with ignore/user exclude rules
/// * `skip_nonregular` - If true, skip directories, symlinks and special files
pub fn build_index_from_scan(
    root: &Path,
    ctx: Arc<ScanContext>,
    skip_nonregular: bool,
) -> Result<(StagedIndex, Option<String>)> {
    // Channel for batches of FileRecord coming from the walker.
    let (file_tx, file_rx) = channel::unbounded::<Vec<FileRecord>>();

    let num_threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    // Spawn the walker on a dedicated thread so we can consume records
    // and feed the builder concurrently.
    let walker_handle = {
        let ctx = Arc::clone(&ctx);
        let root = root.to_path_buf();
        let tx = file_tx.clone();

        thread::spawn(move || walk_parallel(vec![root], tx, ctx, num_threads))
    };

    // This ensures that once the walker
    // and its workers drop their senders, the receiver will see EOF.
    drop(file_tx);

    // IndexBuilder now needs the root path up front.
    let mut builder = IndexBuilder::new(root.to_path_buf());

    // Stream batches into the builder as they arrive.
    while let Ok(batch) = file_rx.recv() {
        if skip_nonregular {
            builder.add_batch(
                batch
                    .into_iter()
                    .filter(|r| !r.is_dir && !r.is_symlink && !r.is_special),
            );
        } else {
            builder.add_batch(batch);
        }
    }

    // Propagate walker errors / panics.
    let walk_result = walker_handle
        .join()
        .map_err(|_| Error::msg("filesystem walker thread panicked"))?;

    // If walk_parallel returns Result<(), Error>, this propagates that error too.
    walk_result?;

    let staged = builder.finish();

    Ok((staged, None))
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
