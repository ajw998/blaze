use std::path::PathBuf;
use std::time::{Duration, Instant};

use blaze_runtime::history::{HistoryStore, QueryEvent};
use chrono::{DateTime, Utc};
use log::debug;

use crate::{
    FileId, IndexReader, Query, QueryEngine, eval::apply_path_order_filter, parse_query, rank,
};
/// Shared, state-independent pipeline context.
struct PipelineCtx<'a, I: IndexReader> {
    /// Underlying index.
    index: &'a I,
    /// Time "now" for ranking decisions.
    now: DateTime<Utc>,
    /// Original query string, if we parsed from text.
    query_str: Option<String>,
    /// Root path for history logging.
    root: Option<PathBuf>,
    /// Total number of logical results (after path-order filter),
    /// even if we only store the top N ranked results.
    result_total: usize,
}

/// Initial state - pipeline created but no query parsed yet.
pub struct InitialState;

/// Query has been parsed, ready for execution.
pub struct ParsedState {
    query: Query,
}

/// Query executed, hits available, ready for ranking.
pub struct ExecutedState {
    query: Query,
    hits: Vec<FileId>,
}

/// Results ranked, ready for consumption.
pub struct RankedState {
    results: Vec<FileId>,
}

/// Stages for which we record timings.
#[derive(Copy, Clone, Debug)]
pub enum Stage {
    Parse,
    Exec,
    Rank,
}

/// Timing metrics collected during pipeline execution.
#[derive(Debug, Clone, Default)]
pub struct PipelineMetrics {
    /// Time spent parsing the query string.
    pub parse_time: Option<Duration>,
    /// Time spent executing the query against the index.
    pub exec_time: Option<Duration>,
    /// Time spent ranking results.
    pub rank_time: Option<Duration>,
}

impl PipelineMetrics {
    /// Total time across all measured stages.
    pub fn total(&self) -> Duration {
        self.parse_time.unwrap_or_default()
            + self.exec_time.unwrap_or_default()
            + self.rank_time.unwrap_or_default()
    }
}

/// Strategy trait for timing behavior.
///
/// Implementations decide whether to measure stages and how to store metrics.
pub trait Timer {
    /// Run `f`, optionally measuring and recording the duration for `stage`.
    fn measure<F, R>(&mut self, stage: Stage, f: F) -> R
    where
        F: FnOnce() -> R;

    /// Return metrics if timing is enabled.
    fn metrics(&self) -> Option<&PipelineMetrics> {
        None
    }
}

/// Timer implementation that does nothing
#[derive(Debug, Default)]
pub struct NoopTimer;

impl Timer for NoopTimer {
    #[inline]
    fn measure<F, R>(&mut self, _stage: Stage, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        // No timing; just run the closure.
        f()
    }

    fn metrics(&self) -> Option<&PipelineMetrics> {
        None
    }
}

/// Timer implementation that collects `PipelineMetrics`.
#[derive(Debug, Default)]
pub struct MetricsTimer {
    metrics: PipelineMetrics,
}

impl MetricsTimer {
    fn new() -> Self {
        Self {
            metrics: PipelineMetrics::default(),
        }
    }
}

impl Timer for MetricsTimer {
    fn measure<F, R>(&mut self, stage: Stage, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let start = Instant::now();
        let result = f();
        let elapsed = start.elapsed();

        match stage {
            Stage::Parse => self.metrics.parse_time = Some(elapsed),
            Stage::Exec => self.metrics.exec_time = Some(elapsed),
            Stage::Rank => self.metrics.rank_time = Some(elapsed),
        }

        result
    }

    fn metrics(&self) -> Option<&PipelineMetrics> {
        Some(&self.metrics)
    }
}

/// A type-safe query execution pipeline.
///
/// Uses typestate pattern to enforce correct ordering of operations
/// at compile time. Timing behavior is controlled by the `Timer`
/// strategy type parameter `T`:
///
/// - `T = NoopTimer`    => untimed pipeline
/// - `T = MetricsTimer` => timed pipeline
pub struct QueryPipeline<'a, I: IndexReader, S, T: Timer = NoopTimer> {
    ctx: PipelineCtx<'a, I>,
    state: S,
    timer: T,
}

impl<'a, I: IndexReader> QueryPipeline<'a, I, InitialState, NoopTimer> {
    /// Create a new pipeline bound to an index (untimed).
    pub fn new(index: &'a I) -> Self {
        Self {
            ctx: PipelineCtx {
                index,
                now: Utc::now(),
                query_str: None,
                root: None,
                result_total: 0,
            },
            state: InitialState,
            timer: NoopTimer::default(),
        }
    }
}
impl<'a, I: IndexReader> QueryPipeline<'a, I, InitialState, MetricsTimer> {
    /// Create a new timed pipeline bound to an index.
    pub fn new_timed(index: &'a I) -> Self {
        Self {
            ctx: PipelineCtx {
                index,
                now: Utc::now(),
                query_str: None,
                root: None,
                result_total: 0,
            },
            state: InitialState,
            timer: MetricsTimer::new(),
        }
    }
}

impl<'a, I: IndexReader, S, T: Timer> QueryPipeline<'a, I, S, T> {
    /// Set the root path for history logging or other higher-level use.
    pub fn with_root(mut self, root: Option<PathBuf>) -> Self {
        self.ctx.root = root;
        self
    }

    /// Access timing metrics, if enabled.
    pub fn metrics(&self) -> Option<&PipelineMetrics> {
        self.timer.metrics()
    }

    /// Get the original query string if this pipeline was created via `parse`.
    pub fn query_str(&self) -> Option<&str> {
        self.ctx.query_str.as_deref()
    }

    /// Get the root path (if any) associated with this query.
    pub fn root(&self) -> Option<&PathBuf> {
        self.ctx.root.as_ref()
    }
}

impl<'a, I: IndexReader, T: Timer> QueryPipeline<'a, I, InitialState, T> {
    /// Parse a query string into a [Query] AST.
    pub fn parse(self, query_str: &str) -> QueryPipeline<'a, I, ParsedState, T> {
        let QueryPipeline {
            mut ctx,
            state: InitialState,
            mut timer,
        } = self;

        let query = timer.measure(Stage::Parse, || parse_query(query_str));
        ctx.query_str = Some(query_str.to_string());

        QueryPipeline {
            ctx,
            state: ParsedState { query },
            timer,
        }
    }

    /// Use a pre-parsed query.
    ///
    /// In this case, no original string is stored for history.
    pub fn with_query(self, query: Query) -> QueryPipeline<'a, I, ParsedState, T> {
        let QueryPipeline {
            ctx,
            state: InitialState,
            timer,
        } = self;

        QueryPipeline {
            ctx,
            state: ParsedState { query },
            timer,
        }
    }
}

impl<'a, I: IndexReader + Sync, T: Timer> QueryPipeline<'a, I, ParsedState, T> {
    /// Execute the query against the index using `QueryEngine`.
    ///
    /// Returns matching file IDs (unranked, in index order).
    pub fn execute(self) -> QueryPipeline<'a, I, ExecutedState, T> {
        let QueryPipeline {
            ctx,
            state: ParsedState { query },
            mut timer,
        } = self;

        let engine = QueryEngine::new(ctx.index);

        // QueryEngine decides how to handle timestamps for predicate evaluation.
        // Ranking uses `ctx.now` separately.
        let hits = timer.measure(Stage::Exec, || engine.eval_query(&query));

        QueryPipeline {
            ctx,
            state: ExecutedState { query, hits },
            timer,
        }
    }

    /// Get a reference to the parsed query.
    pub fn query(&self) -> &Query {
        &self.state.query
    }
}

impl<'a, I: IndexReader, T: Timer> QueryPipeline<'a, I, ExecutedState, T> {
    /// Rank results by relevance with no explicit limit.
    ///
    /// This passes `None` to the ranking engine, which can interpret this
    /// as "unbounded ranking".
    pub fn rank(self, limit: Option<usize>) -> QueryPipeline<'a, I, RankedState, T> {
        self.rank_internal(limit)
    }

    /// Rank results but only keep the top `limit`.
    ///
    /// Still records the total match count (after path-order filtering) so we
    /// can report truncation in the CLI without scoring every file.
    pub fn rank_with_limit(self, limit: Option<usize>) -> QueryPipeline<'a, I, RankedState, T> {
        self.rank_internal(limit)
    }

    /// Internal helper that drives ranking with an optional limit.
    fn rank_internal(self, limit: Option<usize>) -> QueryPipeline<'a, I, RankedState, T> {
        let QueryPipeline {
            mut ctx,
            state: ExecutedState { query, hits },
            mut timer,
        } = self;

        // Apply path-order filter before ranking.
        let filtered = apply_path_order_filter(ctx.index, &query, hits);
        ctx.result_total = filtered.len();

        let index = ctx.index;
        let now = ctx.now;

        let ranked = timer.measure(Stage::Rank, || rank(index, &query, &filtered, now, limit));

        QueryPipeline {
            ctx,
            state: RankedState { results: ranked },
            timer,
        }
    }

    /// Skip ranking and use hits as-is.
    ///
    /// This does *not* apply the path-order filter, by design.
    pub fn unranked(self) -> QueryPipeline<'a, I, RankedState, T> {
        let QueryPipeline {
            mut ctx,
            state: ExecutedState { query: _, hits },
            mut timer,
        } = self;

        let results = timer.measure(Stage::Rank, || hits);
        ctx.result_total = results.len();

        QueryPipeline {
            ctx,
            state: RankedState { results },
            timer,
        }
    }

    /// Get the number of hits before ranking.
    pub fn hit_count(&self) -> usize {
        self.state.hits.len()
    }

    /// Get a reference to the raw hits.
    pub fn hits(&self) -> &[FileId] {
        &self.state.hits
    }
}

impl<'a, I: IndexReader, T: Timer> QueryPipeline<'a, I, RankedState, T> {
    /// Consume the pipeline and return the final results.
    pub fn into_results(self) -> Vec<FileId> {
        self.state.results
    }

    /// Get a reference to the results without consuming the pipeline.
    pub fn results(&self) -> &[FileId] {
        &self.state.results
    }

    /// Get the total number of results after filtering,
    /// not just the number stored (which may be limited by ranking).
    pub fn count(&self) -> usize {
        self.ctx.result_total
    }

    /// Get a reference to the index for path reconstruction.
    pub fn index(&self) -> &'a I {
        self.ctx.index
    }

    /// `reconstruct_full_path` may return absolute or root-relative paths.
    /// If the path is already absolute (starts with `/`), we use it as-is.
    /// Otherwise we prefix with `/` to display a Unix-style absolute path.
    pub fn iter_with_paths(&self) -> impl Iterator<Item = (usize, FileId, String)> + '_ {
        self.state.results.iter().enumerate().map(move |(i, &fid)| {
            let rel_path = self.ctx.index.reconstruct_full_path(fid);

            let display_path = if rel_path.is_empty() {
                "/".to_string()
            } else if rel_path.starts_with('/') {
                rel_path
            } else {
                format!("/{}", rel_path)
            };

            (i + 1, fid, display_path)
        })
    }

    /// Take the top `n` results.
    pub fn take(self, n: usize) -> Vec<FileId> {
        let mut results = self.into_results();
        results.truncate(n);
        results
    }

    /// Log this query execution to history.
    ///
    /// This is best-effort: failures are logged but not propagated.
    /// Requires that `parse()` was called (not `with_query()`), otherwise
    pub fn log_history(&self) {
        let Some(query_str) = self.query_str() else {
            debug!("Cannot log history: no query_str (was with_query() used?)");
            return;
        };

        // Compute total duration in milliseconds, if we have metrics.
        let duration_ms: Option<u32> = self.metrics().map(|m| {
            // total() is a Duration; convert to ms and clamp to u32.
            let ms = m.total().as_secs_f64() * 1000.0;
            ms.round().clamp(0.0, u32::MAX as f64) as u32
        });

        let Some(history) = HistoryStore::new() else {
            debug!("Cannot open history store");
            return;
        };

        let event = QueryEvent::new(
            query_str.to_string(),
            self.count(),
            duration_ms.unwrap_or(0),
        );

        history.log_query(event)
    }
}
