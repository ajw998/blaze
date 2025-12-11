use std::sync::OnceLock;

use chrono::Local;
use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};

use crate::config::PROGRAM_LOG_LEVEL;

enum LogTarget {
    Stderr,
}

pub struct Logger {
    level: Level,
    target: LogTarget,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
            let msg = format!(
                "{} {} [{}] {}",
                timestamp,
                record.level(),
                record.target(),
                record.args()
            );

            match &self.target {
                LogTarget::Stderr => {
                    eprintln!("{msg}")
                }
            }
        }
    }

    fn flush(&self) {}
}

fn get_level_from_env() -> Level {
    std::env::var(PROGRAM_LOG_LEVEL)
        .ok()
        .and_then(|s| s.parse::<LevelFilter>().ok())
        .and_then(|filter| filter.to_level())
        .unwrap_or(Level::Warn)
}

pub fn init() -> Result<(), SetLoggerError> {
    _init(get_level_from_env())
}

pub fn _init(level: Level) -> Result<(), SetLoggerError> {
    static LOGGER: OnceLock<Logger> = OnceLock::new();

    // Check whether it is an initial call,
    // since log::set_max_level uses the pass-in `level` value,
    // so in theory, the initial level at get_or_init and max_level
    // can create a mismatch.
    let init_call = LOGGER.get().is_none();

    let logger = LOGGER.get_or_init(|| Logger {
        level,
        target: LogTarget::Stderr,
    });

    if init_call {
        log::set_logger(logger)?;
        log::set_max_level(level.to_level_filter());
    }

    Ok(())
}

#[cfg(test)]
#[path = "logging_tests.rs"]
mod tests;
