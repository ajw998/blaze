use super::*;

use crossbeam::channel;
use std::{
    fs::{self, create_dir, write},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering as AtomicOrdering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn default_ctx() -> ScanContext {
    ScanContext {
        trash: TrashConfig::default(),
        ignore: IgnoreEngine::default(),
        user_excludes: UserExcludes::default(),
    }
}

#[test]
fn to_unix_secs_handles_none_and_various_times() {
    let cases: &[(Option<SystemTime>, u64)] = &[
        (None, 0),
        (Some(UNIX_EPOCH), 0),
        (Some(UNIX_EPOCH + Duration::from_secs(42)), 42),
        (
            UNIX_EPOCH.checked_sub(Duration::from_secs(1)),
            0, // before epoch => treated as 0
        ),
    ];

    for (input, expected) in cases {
        let got = to_unix_secs(*input);
        assert_eq!(
            got, *expected,
            "to_unix_secs({:?}) should be {}, got {}",
            input, expected, got
        );
    }
}

#[test]
fn inspect_fs_entry_returns_record_for_regular_file() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    let file_path = root.join("file.txt");
    write(&file_path, b"hello world").expect("write file");

    let ctx = default_ctx();

    let mut entries = fs::read_dir(root).expect("read_dir");
    let dir_entry = entries
        .find(|res| {
            res.as_ref()
                .ok()
                .map(|e| e.file_name() == "file.txt")
                .unwrap_or(false)
        })
        .expect("file entry")
        .expect("file entry ok");

    let outcome = inspect_fs_entry(&dir_entry, &ctx)
        .expect("inspect_fs_entry ok")
        .expect("some entry");

    let rec = &outcome;

    assert_eq!(rec.full_path, file_path);
    assert_eq!(rec.name, "file.txt");
    assert_eq!(rec.ext.as_deref(), Some("txt"));
    assert_eq!(rec.size, 11);
    assert!(!rec.is_dir);
    assert!(!rec.is_symlink);
    assert!(!rec.is_special);
    assert!(!rec.hidden_os);
    assert!(!rec.ignored_glob);
    assert!(!rec.user_excludes);
    assert!(!rec.in_trash);
}

#[test]
fn inspect_fs_entry_marks_directories_and_recurse_flag() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    let subdir = root.join("sub");
    create_dir(&subdir).expect("create subdir");

    let ctx = default_ctx();

    let mut entries = fs::read_dir(root).expect("read_dir");
    let dir_entry = entries
        .find(|res| {
            res.as_ref()
                .ok()
                .map(|e| e.file_name() == "sub")
                .unwrap_or(false)
        })
        .expect("subdir entry")
        .expect("subdir entry ok");

    let outcome = inspect_fs_entry(&dir_entry, &ctx)
        .expect("inspect_fs_entry ok")
        .expect("some entry");

    let rec = &outcome;

    assert_eq!(rec.full_path, subdir);
    assert_eq!(rec.name, "sub");
    assert!(rec.is_dir);
    assert!(!rec.is_symlink);
    assert!(!rec.is_special);
    assert_eq!(rec.size, 0);
}

#[test]
fn inspect_fs_entry_marks_hidden_files() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    let hidden_path = root.join(".hidden");
    write(&hidden_path, b"x").expect("write hidden file");

    let ctx = default_ctx();

    let mut entries = fs::read_dir(root).expect("read_dir");
    let dir_entry = entries
        .find(|res| {
            res.as_ref()
                .ok()
                .map(|e| e.file_name() == ".hidden")
                .unwrap_or(false)
        })
        .expect("hidden entry")
        .expect("hidden entry ok");

    let outcome = inspect_fs_entry(&dir_entry, &ctx)
        .expect("inspect_fs_entry ok")
        .expect("some entry");

    let rec = &outcome;

    assert!(rec.hidden_os);
    assert_eq!(rec.name, ".hidden");
}

#[test]
fn scan_dir_parallel_enqueues_subdirs_and_builds_batch() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    // root/
    //   a.txt
    //   sub/
    //     b.txt
    write(root.join("a.txt"), b"a").expect("write a.txt");
    create_dir(root.join("sub")).expect("create sub");
    write(root.join("sub").join("b.txt"), b"b").expect("write b.txt");

    let ctx = default_ctx();
    let (work_tx, work_rx) = channel::unbounded::<PathBuf>();
    let mut batch = Vec::new();
    let pending = AtomicUsize::new(0);

    scan_dir_parallel(root, &work_tx, &mut batch, &ctx, &pending).expect("scan_dir_parallel");

    // Exactly one subdirectory should be enqueued.
    let queued = work_rx.try_recv().expect("a subdir should be queued");
    assert_eq!(queued, root.join("sub"));
    assert!(work_rx.try_recv().is_err(), "only one subdir expected");

    // Batch should contain records for "a.txt" and "sub".
    let mut names: Vec<_> = batch.iter().map(|r| r.name.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["a.txt", "sub"]);

    // Pending should reflect the one enqueued subdir.
    assert_eq!(pending.load(AtomicOrdering::Relaxed), 1);
}

#[test]
fn walk_parallel_scans_tree_and_emits_all_records() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().to_path_buf();

    // root/
    //   a.txt
    //   sub/
    //     b.txt
    write::<_, _>(root.join("a.txt"), b"a").expect("write a.txt");
    create_dir(root.join("sub")).expect("create sub");
    write(root.join("sub").join("b.txt"), b"b").expect("write b.txt");

    let ctx = Arc::new(default_ctx());
    let (file_tx, file_rx) = channel::unbounded::<Vec<FileRecord>>();

    // Use multiple threads to exercise the parallel path.
    walk_parallel(vec![root.clone()], file_tx.clone(), ctx, 4).expect("walk_parallel");

    // Drop our sender so the receiver will eventually see Disconnected
    drop(file_tx);

    let mut records: Vec<FileRecord> = Vec::new();
    while let Ok(batch) = file_rx.recv() {
        records.extend(batch);
    }

    // Collect relative paths of all records for assertion.
    let mut rel_paths: Vec<PathBuf> = records
        .iter()
        .map(|r| r.full_path.strip_prefix(&root).unwrap().to_path_buf())
        .collect();
    rel_paths.sort();

    // Expect:
    //   a.txt
    //   sub        (directory)
    //   sub/b.txt
    let expected = vec![
        PathBuf::from("a.txt"),
        PathBuf::from("sub"),
        PathBuf::from("sub/b.txt"),
    ];
    assert_eq!(rel_paths, expected);
}

#[test]
fn walk_parallel_with_no_roots_emits_nothing() {
    let ctx = Arc::new(default_ctx());
    let (file_tx, file_rx) = channel::unbounded::<Vec<FileRecord>>();

    walk_parallel(Vec::new(), file_tx.clone(), ctx, 4).expect("walk_parallel");

    drop(file_tx);
    // No batches should be received.
    assert!(file_rx.recv().is_err());
}
