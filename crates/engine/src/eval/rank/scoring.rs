use crate::{
    IndexReader,
    eval::rank::{FileFeatures, RankingContext},
    flags::NoiseFlags,
};

/// Exact filename match bonus.
const SCORE_NAME_EXACT: i32 = 120;
/// Filename starts with query term.
const SCORE_NAME_PREFIX: i32 = 80;
/// Filename contains query term (base, adjusted by position).
const SCORE_NAME_CONTAINS_BASE: i32 = 40;
/// Minimum score for substring match.
const SCORE_NAME_CONTAINS_MIN: i32 = 10;

/// Path component exact match.
const SCORE_PATH_COMPONENT: i32 = 30;
/// Path contains term
const SCORE_PATH_CONTAINS: i32 = 15;

/// Recency thresholds (in seconds).
const SECS_PER_DAY: i64 = 86_400;
const SECS_PER_WEEK: i64 = 7 * SECS_PER_DAY;
const SECS_PER_MONTH: i64 = 30 * SECS_PER_DAY;

/// Recency tiers
static RECENCY_TIERS: &[(i64, i32)] = &[
    (SECS_PER_DAY, 40),
    (SECS_PER_WEEK, 25),
    (SECS_PER_MONTH, 10),
];

/// Noise penalties: tuned to be on the same order of magnitude as
/// name/path/recency scores so they meaningfully demote noisy paths.
const PENALTY_SYSTEM_DIR: i32 = 60;
const PENALTY_BUILD_DIR: i32 = 90;
const PENALTY_CACHE_DIR: i32 = 70;
const PENALTY_HASHY_SEG: i32 = 40;
const PENALTY_VERY_DEEP: i32 = 10;
const PENALTY_APP_DATA_DIR: i32 = 50;
const PENALTY_LOG_DIR: i32 = 40;

// Depth at which we start penalising (components, not characters).
const DEPTH_PENALTY_START: u8 = 8;
// Penalty per extra level beyond the start.
const DEPTH_PENALTY_PER_LEVEL: i32 = 2;
// Max magnitude of the depth penalty.
const DEPTH_PENALTY_MAX: i32 = 30;

#[inline]
fn score_path_depth<I: IndexReader>(features: &FileFeatures<'_, I>) -> i32 {
    let depth = features.path_depth() as i32;
    let excess = (depth - DEPTH_PENALTY_START as i32).max(0);
    let penalty = excess * DEPTH_PENALTY_PER_LEVEL;

    // Return a negative score (penalty).
    -penalty.min(DEPTH_PENALTY_MAX)
}
/// Utility to sum scores over query terms while handling the empty-terms case.
#[inline]
fn sum_term_scores(ctx: &RankingContext, mut scorer: impl FnMut(&str) -> i32) -> i32 {
    if ctx.terms.is_empty() {
        return 0;
    }

    ctx.terms.iter().map(|term| scorer(term)).sum()
}

//
// Main scoring functions
//

/// Compute the total relevance score for a file.
///
/// Higher scores indicate more relevant results.
pub(super) fn compute_score<I: IndexReader>(
    features: &mut FileFeatures<'_, I>,
    ctx: &RankingContext,
) -> i32 {
    let mut score = 0;

    score += score_name_match(features, ctx);
    score += score_path_match(features, ctx);
    score += score_recency(features, ctx);
    score += score_path_depth(features);
    score += score_type_category(features);
    score -= noise_penalty(features);

    score
}

/// Compute a quick approximation score using only cheap features.
///
/// This skips expensive operations like name/path matching and only uses:
/// - Recency (cheap: just `modified_epoch`)
/// - File type category (cheap: just extension)
/// - Noise penalty (cheap: pre-computed flags)
pub(super) fn compute_quick_score<I: IndexReader>(
    features: &FileFeatures<'_, I>,
    ctx: &RankingContext,
) -> i32 {
    let mut score = 0;

    // Only use cheap components (no name/path matching).
    score += score_recency(features, ctx);
    score += score_type_category(features);
    score += score_path_depth(features);
    score -= noise_penalty(features);

    score
}

/// Score based on filename matching query terms.
/// Rewards matches in the following descending order:
/// Exact match > Prefix match > Substring match (position-adjusted).
#[inline]
pub(super) fn score_name_match<I: IndexReader>(
    features: &mut FileFeatures<'_, I>,
    ctx: &RankingContext,
) -> i32 {
    if ctx.terms.is_empty() {
        return 0;
    }

    let name_lower = features.name_lower();
    sum_term_scores(ctx, |term| score_term_in_name(name_lower, term))
}

/// Score a single term against a filename.
fn score_term_in_name(name: &str, term: &str) -> i32 {
    if name == term {
        SCORE_NAME_EXACT
    } else if name.starts_with(term) {
        SCORE_NAME_PREFIX
    } else if let Some(pos) = name.find(term) {
        // Earlier position = higher score.
        (SCORE_NAME_CONTAINS_BASE - pos as i32).max(SCORE_NAME_CONTAINS_MIN)
    } else {
        0
    }
}

/// Score based on path matching query terms.
///
/// Checks if query terms appear as path components or substrings.
#[inline]
pub(super) fn score_path_match<I: IndexReader>(
    features: &mut FileFeatures<'_, I>,
    ctx: &RankingContext,
) -> i32 {
    // Only compute path if we have terms (lazy evaluation).
    if ctx.terms.is_empty() {
        return 0;
    }

    let Some(full_path_lower) = features.full_path_lower() else {
        return 0;
    };

    sum_term_scores(ctx, |term| score_term_in_path(full_path_lower, term))
}

/// Score a single term against path components.
fn score_term_in_path(full_path: &str, term: &str) -> i32 {
    if full_path
        .split('/')
        .filter(|component| !component.is_empty())
        .any(|component| component == term)
    {
        SCORE_PATH_COMPONENT
    } else if full_path.contains(term) {
        SCORE_PATH_CONTAINS
    } else {
        0
    }
}

/// Score based on recency of modification.
///
/// More recently modified files get higher scores, but build/cache/app-data/log
/// noise locations do *not* receive recency bonuses.
#[inline]
pub(super) fn score_recency<I: IndexReader>(
    features: &FileFeatures<'_, I>,
    ctx: &RankingContext,
) -> i32 {
    let flags = features.noise_flags();

    // Don't reward recency for typical noisy locations.
    if flags.intersects(
        NoiseFlags::BUILD_DIR
            | NoiseFlags::CACHE_DIR
            | NoiseFlags::APP_DATA_DIR
            | NoiseFlags::LOG_DIR,
    ) {
        return 0;
    }

    let age_secs = ctx.now.timestamp() - features.modified_epoch();

    RECENCY_TIERS
        .iter()
        .find(|(max_age, _)| age_secs < *max_age)
        .map(|(_, score)| *score)
        .unwrap_or(0)
}

/// Score based on file type category.
///
/// Documents and code files are boosted; binaries are penalized.
///
/// Blaze is an opinionated tool rather than a generic library, we
/// hardcode categories via a `match` on the extension for speed and clarity.
/// In noisy locations (build/cache/app-data/log/system dirs), the type signal
/// is downweighted so that e.g. `target/.../*.rs` doesn't compete with real
/// project sources. Obviously we need to expand on this...
#[inline]
pub(super) fn score_type_category<I: IndexReader>(features: &FileFeatures<'_, I>) -> i32 {
    let base = match features.ext() {
        // Documents
        "pdf" | "doc" | "docx" | "txt" | "md" | "rst" | "rtf" | "odt" => 20,

        // Code
        "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "go" | "java" | "c" | "cpp" | "h" | "hpp"
        | "rb" | "php" | "swift" | "kt" | "scala" | "hs" | "ml" | "ex" | "exs" | "clj" | "cs"
        | "fs" | "lua" | "sh" | "bash" | "zsh" | "fish" | "pl" | "r" | "sql" | "zig" | "nim"
        | "v" | "d" | "cr" => 15,

        // Config
        "json" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" | "xml" | "env" => 5,

        // Binary / compiled (negative score)
        "exe" | "dll" | "so" | "dylib" | "o" | "a" | "lib" | "bin" | "class" | "pyc" | "pyo"
        | "wasm" => -20,

        _ => 0,
    };

    let flags = features.noise_flags();

    // Downweight type bonuses in noisy locations.
    if flags.intersects(
        NoiseFlags::BUILD_DIR
            | NoiseFlags::CACHE_DIR
            | NoiseFlags::APP_DATA_DIR
            | NoiseFlags::LOG_DIR
            | NoiseFlags::SYSTEM_DIR,
    ) {
        base / 3
    } else {
        base
    }
}

/// Calculate penalty for noisy/less relevant files.
///
/// Uses pre-computed noise flags from the index for efficiency.
/// Penalizes:
/// - System directories (/usr, /lib, /etc)
/// - Build/dependency directories (e.g. node_modules)
/// - Cache directories (.cache, __pycache__)
/// - Hash-like path segments (generated content)
/// - Deeply nested paths
/// - Application data directories
/// - Log/debug directories
#[inline]
pub(super) fn noise_penalty<I: IndexReader>(features: &FileFeatures<'_, I>) -> i32 {
    let flags = features.noise_flags();
    let mut penalty = 0;

    if flags.contains(NoiseFlags::SYSTEM_DIR) {
        penalty += PENALTY_SYSTEM_DIR;
    }
    if flags.contains(NoiseFlags::BUILD_DIR) {
        penalty += PENALTY_BUILD_DIR;
    }
    if flags.contains(NoiseFlags::CACHE_DIR) {
        penalty += PENALTY_CACHE_DIR;
    }
    if flags.contains(NoiseFlags::HASHY_SEG) {
        penalty += PENALTY_HASHY_SEG;
    }
    if flags.contains(NoiseFlags::VERY_DEEP) {
        penalty += PENALTY_VERY_DEEP;
    }
    if flags.contains(NoiseFlags::APP_DATA_DIR) {
        penalty += PENALTY_APP_DATA_DIR;
    }
    if flags.contains(NoiseFlags::LOG_DIR) {
        penalty += PENALTY_LOG_DIR;
    }

    penalty
}
