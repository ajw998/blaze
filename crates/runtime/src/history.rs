use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use log::debug;
use serde::{Deserialize, Serialize};

pub const HISTORY_VERSION: u8 = 1;

pub const HISTORY_DISABLED_ENV: &str = "BLAZE_HISTORY";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum HistoryEvent {
    Query(QueryEvent),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryEvent {
    /// Schema version
    pub version: u8,

    /// Timestamp
    pub timestamp: DateTime<Utc>,

    /// Raw query string
    pub raw_query: String,

    /// Number of results returned.
    pub hits: usize,

    /// Query execution time in milliseconds.
    pub duration_ms: u32,
}

impl QueryEvent {
    pub fn new(raw_query: String, hits: usize, duration_ms: u32) -> Self {
        Self {
            version: HISTORY_VERSION,
            timestamp: Utc::now(),
            raw_query,
            hits,
            duration_ms,
        }
    }
}

pub struct HistoryStore {
    path: PathBuf,
}

pub fn state_dir() -> Option<PathBuf> {
    // Check XDG_STATE_HOME first (Linux)
    if let Ok(xdg_state) = env::var("XDG_STATE_HOME")
        && !xdg_state.is_empty()
    {
        return Some(PathBuf::from(xdg_state).join("blaze"));
    }

    // Fall back to dirs crate
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .map(|p| p.join("blaze"))
}

pub fn history_log_path() -> Option<PathBuf> {
    state_dir().map(|d| d.join("history.log"))
}

fn history_disabled() -> bool {
    match env::var(HISTORY_DISABLED_ENV) {
        Ok(val) => val == "0" || val.eq_ignore_ascii_case("false"),
        Err(_) => false,
    }
}

impl HistoryStore {
    // TODO: Use different history path
    pub fn new() -> Option<Self> {
        if history_disabled() {
            return None;
        }

        let path = history_log_path()?;
        Some(Self { path })
    }

    /// Create a history store with a custom path (for testing).
    #[cfg(test)]
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn log_query(&self, event: QueryEvent) {
        if let Err(e) = self.append_event(&HistoryEvent::Query(event)) {
            debug!("Failed to log history event: {}", e);
        }
    }

    fn append_event(&self, event: &HistoryEvent) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut line = serde_json::to_string(event).map_err(io::Error::other)?;
        line.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        // We write a single line-encoded JSON event and rely on O_APPEND so that each individual write call appends atomically.
        // However, because write_all may perform multiple write calls in case of interruption, full-line atomicity is not guaranteed under all failure modes.
        // In practice, this is acceptable for a best-effort history log
        file.write_all(line.as_bytes())?;

        Ok(())
    }

    pub fn iter_events(&self) -> impl Iterator<Item = HistoryEvent> {
        self.read_events().into_iter().flatten()
    }

    fn read_events(&self) -> Option<Vec<HistoryEvent>> {
        let file = File::open(&self.path).ok()?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for line in reader.lines() {
            match line {
                Ok(line) => match serde_json::from_str(&line) {
                    Ok(ev) => events.push(ev),
                    Err(e) => debug!("Skipping malformed history line: {e}"),
                },
                Err(e) => {
                    debug!("Error reading history log: {e}");
                    break;
                }
            }
        }
        Some(events)
    }

    pub fn recent_queries(&self, limit: usize) -> Vec<QueryEvent> {
        let mut queries: Vec<QueryEvent> = self
            .iter_events()
            .map(|e| match e {
                HistoryEvent::Query(q) => q,
            })
            .collect();

        queries.reverse();
        queries.truncate(limit);
        queries
    }

    pub fn count(&self) -> usize {
        self.iter_events().count()
    }

    pub fn clear(&self) -> io::Result<()> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
