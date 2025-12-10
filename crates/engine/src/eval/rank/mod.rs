mod path_order;
mod scoring;

use chrono::{DateTime, Utc};

pub use path_order::apply_path_order_filter;

use crate::{FileId, IndexReader, LeafExpr, Query, QueryExpr, flags::NoiseFlags};

/**
Extracted features for a single file, used during ranking.

Uses lazy evaluation to avoid expensive operations (like path reconstruction
and lowercasing) when they're not needed. For predicate-only queries (e.g.,
`modified:today`), this avoids all string allocations.
*/
struct FileFeatures<'a, I: IndexReader> {
    /// File extension (cheap to get, stored directly).
    ext: &'a str,
    /// The file ID in the index.
    fid: FileId,
    /// Cached lowercase full path (computed on first access).
    full_path_lower: Option<String>,
    /// Reference to the index for lazy lookups.
    index: &'a I,
    /// Last modified time as Unix epoch seconds.
    modified_epoch: i64,
    /// Cached lowercase filename (computed on first access).
    name_lower: Option<String>,
    /// Pre-computed noise classification flags.
    noise_flags: NoiseFlags,
    /// Pre-computed path depth.
    path_depth: u8,
}

impl<'a, I: IndexReader> FileFeatures<'a, I> {
    /// Extract features for a file from the index.
    #[inline]
    pub fn extract(index: &'a I, fid: FileId) -> Self {
        Self {
            index,
            fid,
            name_lower: None,
            full_path_lower: None,
            ext: index.get_file_ext(fid),
            modified_epoch: index.get_file_modified_epoch(fid),
            noise_flags: index.get_file_noise_bits(fid),
            path_depth: index.get_file_path_depth(fid),
        }
    }

    /// Get the file extension.
    #[inline]
    pub fn ext(&self) -> &str {
        self.ext
    }

    /// Get the last modified time as Unix epoch seconds.
    #[inline]
    pub fn modified_epoch(&self) -> i64 {
        self.modified_epoch
    }

    /// Get the pre-computed noise classification flags.
    #[inline]
    pub fn noise_flags(&self) -> NoiseFlags {
        self.noise_flags
    }

    /// Get the pre-computed path depth.
    #[inline]
    pub fn path_depth(&self) -> u8 {
        self.path_depth
    }

    /// Get the lowercase filename, computing it lazily.
    #[inline]
    pub fn name_lower(&mut self) -> &str {
        if self.name_lower.is_none() {
            let name = self.index.get_file_name(self.fid);
            self.name_lower = Some(name.to_lowercase());
        }
        self.name_lower.as_ref().unwrap()
    }

    /// Get the lowercase full path, computing it lazily.
    #[inline]
    pub fn full_path_lower(&mut self) -> Option<&str> {
        if self.full_path_lower.is_none() {
            let full_path = self.index.reconstruct_full_path(self.fid);
            let full_path_lower = full_path.to_lowercase();
            self.full_path_lower = Some(full_path_lower);
        }
        self.full_path_lower.as_deref()
    }
}

pub struct RankingContext {
    /// Text terms extracted from the query, lowercased for matching.
    pub terms: Vec<String>,
    /// Current time for recency scoring.
    pub now: DateTime<Utc>,
}

impl RankingContext {
    /// Create a new ranking context from a query.
    pub fn from_query(query: &Query, now: DateTime<Utc>) -> Self {
        let mut terms = Vec::new();
        collect_text_terms(&query.expr, &mut terms);
        Self { terms, now }
    }
}

/// Rank a set of file IDs by relevance.
///
/// This is the main entry point for ranking. It:
/// 1. Extracts features for each hit (lazily where possible)
/// 2. Computes a score for each file
/// 3. Returns top results sorted by score (descending)
///
/// `limit = None` means "no explicit limit" (return all hits, ranked).
/// `limit = Some(0)` returns an empty result immediately.
pub fn rank<I: IndexReader>(
    index: &I,
    query: &Query,
    hits: &[FileId],
    now: DateTime<Utc>,
    limit: Option<usize>,
) -> Vec<FileId> {
    if hits.is_empty() {
        return Vec::new();
    }

    let ctx = RankingContext::from_query(query, now);

    let effective_limit = match limit {
        None => hits.len(),
        Some(0) => return Vec::new(),
        Some(n) => n.min(hits.len()),
    };

    // Two-pass optimization: for large result sets with small limits,
    // use cheap quick scoring to filter before expensive full scoring.
    const TWO_PASS_THRESHOLD: usize = 1000;
    const TWO_PASS_RATIO: usize = 10; // hits / limit ratio

    if hits.len() > TWO_PASS_THRESHOLD && hits.len() / effective_limit > TWO_PASS_RATIO {
        return rank_two_pass(index, &ctx, hits, effective_limit);
    }

    // Single-pass ranking: extract features and compute full scores.
    let mut scored: Vec<(FileId, i32)> = hits
        .iter()
        .map(|&fid| {
            let mut features = FileFeatures::extract(index, fid);
            let score = scoring::compute_score(&mut features, &ctx);
            (fid, score)
        })
        .collect();

    // Use partial sort if we only need top N results.
    if effective_limit < scored.len() / 2 {
        // Partial sort: O(n + k log k) instead of O(n log n).
        scored.select_nth_unstable_by(effective_limit, |a, b| {
            b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))
        });
        scored.truncate(effective_limit);
        // The prefix is unordered after select_nth, so sort it.
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    } else {
        // Full sort when limit is large relative to hits.
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        scored.truncate(effective_limit);
    }

    scored.into_iter().map(|(fid, _)| fid).collect()
}

/// Two-pass ranking: quick score all, then full score only top candidates.
///
/// For large result sets (e.g., 679K files), this avoids extracting expensive
/// features (name/path) for files that won't be in the top results.
///
/// Pass 1: Quick score all files using only cheap features (O(n))
/// Pass 2: Full score top K*3 candidates with name/path matching (O(k))
fn rank_two_pass<I: IndexReader>(
    index: &I,
    ctx: &RankingContext,
    hits: &[FileId],
    limit: usize,
) -> Vec<FileId> {
    // Pass 1: Quick score all files using cheap features only.
    let mut quick_scored: Vec<(FileId, i32)> = hits
        .iter()
        .map(|&fid| {
            let features = FileFeatures::extract(index, fid);
            let score = scoring::compute_quick_score(&features, ctx);
            (fid, score)
        })
        .collect();

    // Select top candidates with buffer (3x limit to ensure we don't miss good matches).
    let candidate_limit = (limit * 3).min(quick_scored.len());
    quick_scored.select_nth_unstable_by(candidate_limit, |a, b| {
        b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))
    });
    quick_scored.truncate(candidate_limit);

    // Pass 2: Full score only the top candidates.
    let mut fully_scored: Vec<(FileId, i32)> = quick_scored
        .into_iter()
        .map(|(fid, _quick_score)| {
            let mut features = FileFeatures::extract(index, fid);
            let score = scoring::compute_score(&mut features, ctx);
            (fid, score)
        })
        .collect();

    // Final sort and limit.
    fully_scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    fully_scored.truncate(limit);

    fully_scored.into_iter().map(|(fid, _)| fid).collect()
}

/// Recursively collect text terms from a query expression.
/// Terms are lowercased here so we avoid a second allocation pass.
fn collect_text_terms(expr: &QueryExpr, out: &mut Vec<String>) {
    match expr {
        QueryExpr::And(children) | QueryExpr::Or(children) => {
            for child in children {
                collect_text_terms(child, out);
            }
        }
        QueryExpr::Not(inner) => {
            collect_text_terms(inner, out);
        }
        QueryExpr::Leaf(LeafExpr::Text(term)) => {
            if !term.text.is_empty() {
                out.push(term.text.to_lowercase());
            }
        }
        QueryExpr::Leaf(_) => {}
    }
}
