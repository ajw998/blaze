use super::*;
use serial_test::serial;
use tempfile::tempdir;

fn temp_store() -> (HistoryStore, tempfile::TempDir) {
    let dir = tempdir().expect("create temp dir");
    let path = dir.path().join("history.log");
    let store = HistoryStore::with_path(path);
    (store, dir)
}

#[test]
fn query_event_new_sets_fields() {
    let raw = "foo bar".to_string();
    let hits = 42;
    let duration_ms = 17;

    let before = Utc::now();
    let ev = QueryEvent::new(raw.clone(), hits, duration_ms);
    let after = Utc::now();

    assert_eq!(ev.version, HISTORY_VERSION);
    assert_eq!(ev.raw_query, raw);
    assert_eq!(ev.hits, hits);
    assert_eq!(ev.duration_ms, duration_ms);

    // Timestamp should be between before and after (up to clock drift).
    assert!(ev.timestamp >= before && ev.timestamp <= after);
}

#[test]
fn log_and_iter_round_trip_single_event() {
    let (store, _dir) = temp_store();

    let ev = QueryEvent::new("search1".into(), 5, 3);
    store.log_query(ev.clone());

    let events: Vec<HistoryEvent> = store.iter_events().collect();
    assert_eq!(events.len(), 1);

    match &events[0] {
        HistoryEvent::Query(q) => {
            assert_eq!(q.raw_query, ev.raw_query);
            assert_eq!(q.hits, ev.hits);
            assert_eq!(q.duration_ms, ev.duration_ms);
            assert_eq!(q.version, HISTORY_VERSION);
        }
    }
}

#[test]
fn iter_events_empty_when_file_missing() {
    let (store, _dir) = temp_store();
    assert_eq!(store.count(), 0);
    assert_eq!(store.iter_events().count(), 0);
}

#[test]
fn count_matches_number_of_events() {
    let (store, _dir) = temp_store();

    assert_eq!(store.count(), 0);

    store.log_query(QueryEvent::new("q1".into(), 1, 10));
    assert_eq!(store.count(), 1);

    store.log_query(QueryEvent::new("q2".into(), 2, 20));
    store.log_query(QueryEvent::new("q3".into(), 3, 30));
    assert_eq!(store.count(), 3);
}

#[test]
fn clear_removes_file_and_is_idempotent() {
    let (store, _dir) = temp_store();
    let path = store.path().to_path_buf();

    // Ensure file exists.
    store.log_query(QueryEvent::new("q".into(), 1, 1));
    assert!(path.exists());

    // Clear and remove file
    store.clear().expect("clear should succeed");
    assert!(!path.exists());

    // Second clear should still succeed and keep file absent
    store.clear().expect("clear should be idempotent");
    assert!(!path.exists());
}

#[test]
fn malformed_lines_are_skipped() {
    use std::io::Write as _;

    let (store, _dir) = temp_store();
    let path = store.path().to_path_buf();

    // Write a malformed line manually.
    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .expect("open history file for malformed write");
        writeln!(file, "this is not json").unwrap();
    }

    let valid = QueryEvent::new("ok".into(), 1, 1);
    store.log_query(valid.clone());

    // Expect only the valid line to survive
    let events: Vec<HistoryEvent> = store.iter_events().collect();
    assert_eq!(events.len(), 1);

    match &events[0] {
        HistoryEvent::Query(q) => {
            assert_eq!(q.raw_query, valid.raw_query);
            assert_eq!(q.hits, valid.hits);
            assert_eq!(q.duration_ms, valid.duration_ms);
        }
    }
}

#[test]
#[serial]
fn new_respects_history_disabled_env_zero() {
    unsafe { std::env::remove_var(HISTORY_DISABLED_ENV) };
    assert!(
        HistoryStore::new().is_some(),
        "history should be enabled by default"
    );

    unsafe { std::env::set_var(HISTORY_DISABLED_ENV, "0") };
    assert!(
        HistoryStore::new().is_none(),
        "history should be disabled when env is 0"
    );
}

#[test]
#[serial]
fn new_respects_history_disabled_env_false() {
    unsafe { std::env::set_var(HISTORY_DISABLED_ENV, "false") };
    assert!(
        HistoryStore::new().is_none(),
        "history should be disabled when env is false"
    );

    unsafe { std::env::set_var(HISTORY_DISABLED_ENV, "TRUE") };
    assert!(HistoryStore::new().is_some());
    unsafe { std::env::remove_var(HISTORY_DISABLED_ENV) };
}
