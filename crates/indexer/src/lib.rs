use std::{path::Path, sync::Arc, thread};

use anyhow::{Context, Error, Result};
use blaze_engine::{Index, IndexBuilder, StagedIndex, write_index_atomic};
use blaze_fs::{FileRecord, IgnoreEngine, ScanContext, TrashConfig, UserExcludes, walk_parallel};
use crossbeam::channel;

pub fn create_scan_context() -> Result<Arc<ScanContext>> {
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
pub fn build_index_from_scan(
    root: &Path,
    ctx: Arc<ScanContext>,
    skip_nonregular: bool,
) -> Result<(StagedIndex, Option<String>)> {
    let (file_tx, file_rx) = channel::unbounded::<Vec<FileRecord>>();

    let num_threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    let walker_handle = {
        let ctx = Arc::clone(&ctx);
        let root = root.to_path_buf();
        let tx = file_tx.clone();

        thread::spawn(move || walk_parallel(vec![root], tx, ctx, num_threads))
    };

    drop(file_tx);

    let mut builder = IndexBuilder::new(root.to_path_buf());

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

    let walk_result = walker_handle
        .join()
        .map_err(|_| Error::msg("filesystem walker thread panicked"))?;
    walk_result?;

    let staged = builder.finish();

    Ok((staged, None))
}

/// Build an index on disk and then open it.
pub fn build_initial_index(
    root: &Path,
    index_path: &Path,
    skip_nonregular: bool,
) -> Result<(Index, Option<String>)> {
    let scan_context = create_scan_context()?;
    let (staged, atime_warning) = build_index_from_scan(root, scan_context, skip_nonregular)?;

    write_index_atomic(index_path, &staged, 0)
        .with_context(|| format!("Failed to write index to {}", index_path.display()))?;

    let idx = Index::open(index_path).with_context(|| {
        format!(
            "Failed to open freshly written index at {}",
            index_path.display()
        )
    })?;

    Ok((idx, atime_warning))
}

/// Open an existing index, or build a new one if it does not exist.
pub fn open_or_build_index(
    root: &Path,
    index_path: &Path,
    skip_nonregular: bool,
) -> Result<(Index, Option<String>)> {
    if index_path.exists() {
        let idx = Index::open(index_path)
            .with_context(|| format!("Failed to open index at {}", index_path.display()))?;
        Ok((idx, None))
    } else {
        build_initial_index(root, index_path, skip_nonregular)
    }
}
