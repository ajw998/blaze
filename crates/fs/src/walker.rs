use std::{
    fs::{self, read_dir},
    io::Result,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crossbeam::channel::{self, RecvTimeoutError, Sender};
use log::{debug, warn};

use crate::{
    config::BATCH_SIZE,
    excludes::{IgnoreEngine, TrashConfig, UserExcludes},
    record::FileRecord,
};

pub struct ScanContext {
    pub trash: TrashConfig,
    pub ignore: IgnoreEngine,
    pub user_excludes: UserExcludes,
}

/// Multi-threaded parallel walk using crossbeam for improved performance.
///
/// Uses a work-stealing approach where multiple threads process directories
/// concurrently. Records are batched before sending to reduce channel overhead.
pub fn walk_parallel(
    roots: Vec<PathBuf>,
    file_tx: Sender<Vec<FileRecord>>,
    ctx: Arc<ScanContext>,
    num_threads: usize,
) -> Result<()> {
    let (work_tx, work_rx) = channel::unbounded::<PathBuf>();

    // Track pending work items to know when to terminate
    let pending = Arc::new(AtomicUsize::new(roots.len()));

    // Seed work queue with roots
    for root in roots {
        let _ = work_tx.send(root);
    }

    debug!("[walk_parallel] starting with {} threads", num_threads);

    thread::scope(|s| {
        for _thread_id in 0..num_threads {
            let work_rx = work_rx.clone();
            let work_tx = work_tx.clone();
            let file_tx = file_tx.clone();
            let ctx = Arc::clone(&ctx);
            let pending = Arc::clone(&pending);

            s.spawn(move || {
                worker_loop(work_rx, work_tx, file_tx, &ctx, &pending);
            });
        }
    });

    Ok(())
}

/// Worker loop for parallel walking.
/// Each worker processes directories from the work queue and sends batched records.
fn worker_loop(
    work_rx: channel::Receiver<PathBuf>,
    work_tx: channel::Sender<PathBuf>,
    file_tx: Sender<Vec<FileRecord>>,
    ctx: &ScanContext,
    pending: &AtomicUsize,
) {
    let mut batch = Vec::with_capacity(BATCH_SIZE);

    loop {
        // Use timeout to periodically check if all work is done
        match work_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(dir) => {
                if let Err(e) = scan_dir_parallel(&dir, &work_tx, &mut batch, ctx, pending) {
                    warn!("[worker] scan_dir_parallel({:?}) failed: {e}", dir);
                }
                // Send batch if it's full
                if batch.len() >= BATCH_SIZE {
                    let to_send = std::mem::take(&mut batch);
                    if file_tx.send(to_send).is_err() {
                        return;
                    }
                }

                // Decrement pending counter after processing directory
                if pending.fetch_sub(1, Ordering::AcqRel) == 1 {
                    // Last item! Done!
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                // Check if all work is done
                if pending.load(Ordering::Acquire) == 0 {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }

    // Send any remaining records
    if !batch.is_empty() {
        let _ = file_tx.send(batch);
    }
}

/// Scan a directory for the parallel walker.
/// Pushes subdirectories to the work queue and collects records in a batch.
fn scan_dir_parallel(
    dir: &Path,
    work_tx: &channel::Sender<PathBuf>,
    batch: &mut Vec<FileRecord>,
    ctx: &ScanContext,
    pending: &AtomicUsize,
) -> Result<()> {
    let rd = match read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            warn!("[walk] read_dir({:?}) failed: {e}", dir);
            return Ok(());
        }
    };

    for entry_res in rd {
        let entry = match entry_res {
            Ok(e) => e,
            Err(e) => {
                warn!("[walk] error reading entry in {:?}: {e}", dir);
                continue;
            }
        };

        match inspect_fs_entry(&entry, ctx) {
            Ok(Some(outcome)) => {
                if should_recurse(&outcome) {
                    // Increment pending count before sending subdirectory
                    pending.fetch_add(1, Ordering::AcqRel);
                    // Send subdirectory to work queue for parallel processing
                    let _ = work_tx.send(outcome.full_path.clone());
                }
                batch.push(outcome);
            }
            Ok(None) => {}
            Err(e) => {
                warn!("[walk] inspect_entry error in {:?}: {e}", dir);
            }
        }
    }

    Ok(())
}

fn should_recurse(f: &FileRecord) -> bool {
    // Determine if we should recurse into this directory
    f.is_dir && !f.in_trash && !f.ignored_glob && !f.user_excludes && !f.is_symlink
}

fn inspect_fs_entry(entry: &fs::DirEntry, ctx: &ScanContext) -> Result<Option<FileRecord>> {
    let metadata = entry.metadata()?;
    let full_path = entry.path();

    let is_dir = metadata.is_dir();
    let is_symlink = metadata.is_symlink();
    let is_file = metadata.is_file();
    let is_special = !is_dir && !is_symlink && !is_file;

    let name_os = entry.file_name();
    let name = match name_os.to_str() {
        Some(s) => s.to_owned(),
        None => return Ok(None),
    };

    let hidden_os = name.starts_with('.');
    let in_trash = ctx.trash.is_in_trash(&full_path);
    let ignored_glob = ctx.ignore.is_ignored(&full_path, is_dir);
    let user_excludes = ctx.user_excludes.is_excluded(&full_path);

    // Reuse metadata - no second syscall needed
    // The issue here is that, on many UNIX systems, created time can
    // fail or return something we don't think as "creation time". The following
    // defaults to 0, which basically means either 1970-01-01, or permission error,
    // or filesystems that don't support creation time. We might need to change
    // FileRecord to use Option<u64> instead
    let (size, mtime_secs, ctime_secs, atime_secs) = if is_dir {
        (0, 0, 0, 0)
    } else {
        let size = metadata.len();
        let mtime_secs = to_unix_secs(metadata.modified().ok());
        let ctime_secs = to_unix_secs(metadata.created().ok());
        let atime_secs = to_unix_secs(metadata.accessed().ok());

        (size, mtime_secs, ctime_secs, atime_secs)
    };

    let extension = entry
        .path()
        .extension()
        .and_then(|os| os.to_str())
        .map(|s| s.to_ascii_lowercase());

    Ok(Some(FileRecord {
        full_path,
        name,
        size,
        mtime_secs,
        ctime_secs,
        atime_secs,
        ignored_glob,
        ext: extension,
        user_excludes,
        is_dir,
        is_symlink,
        is_special,
        in_trash,
        hidden_os,
    }))
}

fn to_unix_secs(t: Option<SystemTime>) -> u64 {
    t.and_then(|tt| tt.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "walker_tests.rs"]
mod tests;
