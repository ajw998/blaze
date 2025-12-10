use super::*;
use log::{Level, Metadata, Record};
use std::fs::File;
use std::sync::Mutex;
use tempfile::tempdir;

#[test]
fn get_level_from_env_parses_cases() {
    let cases: &[(Option<&str>, Level)] = &[
        (None, Level::Warn),
        (Some("debug"), Level::Debug),
        (Some("DEBUG"), Level::Debug),
        (Some("info"), Level::Info),
        (Some("INFO"), Level::Info),
        (Some("warn"), Level::Warn),
        (Some("WARN"), Level::Warn),
        (Some("error"), Level::Error),
        (Some("ERROR"), Level::Error),
        (Some("trace"), Level::Trace),
        (Some("TRACE"), Level::Trace),
        (Some("garbage"), Level::Warn),
        (Some("off"), Level::Warn),
    ];

    for (value, expected) in cases {
        match value {
            Some(v) => unsafe { std::env::set_var(PROGRAM_LOG_LEVEL, v) },
            None => unsafe { std::env::remove_var(PROGRAM_LOG_LEVEL) },
        }

        let lvl = get_level_from_env();
        assert_eq!(
            lvl, *expected,
            "env {:?} should yield level {:?}, got {:?}",
            value, expected, lvl
        );
    }

    unsafe { std::env::remove_var(PROGRAM_LOG_LEVEL) };
}

#[test]
fn enabled_respects_level_threshold() {
    let levels = [
        Level::Error,
        Level::Warn,
        Level::Info,
        Level::Debug,
        Level::Trace,
    ];

    for logger_level in levels {
        let logger = Logger {
            level: logger_level,
            target: LogTarget::Stderr,
        };

        for record_level in levels {
            let meta = Metadata::builder()
                .level(record_level)
                .target("test_target")
                .build();

            let expected = record_level <= logger_level;
            assert_eq!(
                logger.enabled(&meta),
                expected,
                "logger level {:?}, record level {:?}",
                logger_level,
                record_level
            );
        }
    }
}

#[test]
fn file_logger_expected_format() {
    let dir = tempdir().expect("create temp dir");
    let path = dir.path().join("log.txt");
    let file = File::create(&path).expect("create log file");

    let logger = Logger {
        level: Level::Info,
        target: LogTarget::File(Mutex::new(file)),
    };

    let record = Record::builder()
        .level(Level::Info)
        .target("my_target")
        .args(format_args!("hello world"))
        .build();

    logger.log(&record);
    logger.flush();

    let contents = std::fs::read_to_string(&path).expect("read log file");

    assert!(contents.contains("INFO"));
    assert!(contents.contains("[my_target]"));
    assert!(contents.contains("hello world"));
}

#[test]
fn file_logger_respects_level_filter() {
    let dir = tempdir().expect("create temp dir");
    let path = dir.path().join("log.txt");
    let file = File::create(&path).expect("create log file");

    let logger = Logger {
        level: Level::Warn,
        target: LogTarget::File(Mutex::new(file)),
    };

    // All these are below WARN and should be ignored.
    let below = [
        (Level::Info, "info msg"),
        (Level::Debug, "debug msg"),
        (Level::Trace, "trace msg"),
    ];

    for (lvl, msg) in &below {
        let args = format_args!("{msg}");
        let record = Record::builder().level(*lvl).target("t").args(args).build();
        logger.log(&record);
    }
    logger.flush();

    let contents = std::fs::read_to_string(&path).expect("read log file");
    assert!(
        contents.is_empty(),
        "no lines should have been written for below-level records, got: {contents:?}"
    );
}

#[test]
fn stderr_logger_does_not_panic() {
    let logger = Logger {
        level: Level::Info,
        target: LogTarget::Stderr,
    };

    let cases = [
        (Level::Debug, "debug"),
        (Level::Info, "info"),
        (Level::Error, "error"),
    ];

    for (lvl, msg) in &cases {
        let args = format_args!("{msg}");
        let record = Record::builder().level(*lvl).target("t").args(args).build();
        logger.log(&record);
    }

    logger.flush();
}
