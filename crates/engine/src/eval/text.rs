use smallvec::SmallVec;

use crate::{
    FileId, IndexReader, TextTerm, Trigram, build_trigrams_for_string,
    eval::helpers::intersect_adaptive_into, intersect_adaptive,
};

/// How many candidates are "small enough" to skip trigram intersection.
const SMALL_CANDIDATE_CUTOFF: usize = 2_000;
/// When current trigram-filtered candidate set is <= this, stop intersecting further trigrams
/// and go straight to full verification.
const EARLY_VERIFY_CUTOFF: usize = 256;
/// Skip trigrams that hit more than this fraction of all files (too common).
const MAX_TRIGRAM_GLOBAL_SHARE: f64 = 0.30;
/// Maximum number of trigrams to use per query.
/// Using only the rarest N trigrams gives most of the filtering power.
const MAX_TRIGRAMS_PER_QUERY: usize = 3;

/// State derived from a single text term.
struct TextSearchState {
    /// Lowercased search term (typically the last path segment).
    needle_lower: String,
    /// Pre-computed trigrams for the term.
    trigrams: Vec<Trigram>,
}

impl TextSearchState {
    fn new(term: &TextTerm) -> Self {
        let search = extract_search_term(&term.text);
        let trigrams = build_trigrams_for_string(search);

        Self {
            needle_lower: search.to_lowercase(),
            trigrams,
        }
    }

    #[inline]
    fn is_trigram_capable(&self) -> bool {
        !self.trigrams.is_empty()
    }
}

/// Case-insensitive substring match optimized for ASCII haystacks.
///
/// `needle_lower` must already be lowercased.
#[inline]
fn contains_lowercase_ascii(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }

    if haystack.is_ascii() {
        let h = haystack.as_bytes();
        let n = needle_lower.as_bytes();

        if n.len() > h.len() {
            return false;
        }

        'outer: for start in 0..=(h.len() - n.len()) {
            for (i, &nb) in n.iter().enumerate() {
                if h[start + i].to_ascii_lowercase() != nb {
                    continue 'outer;
                }
            }
            return true;
        }
        false
    } else {
        // Slow path: full Unicode case folding.
        haystack.to_lowercase().contains(needle_lower)
    }
}

/// If the input is `commands/query.rs`, treat the intent as "query.rs".
#[inline]
pub fn extract_search_term(text: &str) -> &str {
    match text.rfind('/') {
        Some(pos) => &text[pos + 1..],
        None => text,
    }
}

/// Evaluate a single text term against the index using full-path trigram filtering.
///
/// Returns a *sorted* subset of `candidates`.
pub fn eval_text_term<I: IndexReader>(
    index: &I,
    term: &TextTerm,
    candidates: &[FileId],
) -> Vec<FileId> {
    let state = TextSearchState::new(term);
    eval_text_base_with_state(index, &state, candidates)
}

/// Filter candidates by checking *all* text terms in a single pass.
///
/// Used by the pure-text AND optimisation:
/// 1. Seed from one term via trigrams.
/// 2. Verify all terms against each candidate path once.
pub fn filter_candidates_by_all_terms<I: IndexReader>(
    index: &I,
    terms: &[&TextTerm],
    candidates: &[FileId],
) -> Vec<FileId> {
    if candidates.is_empty() || terms.is_empty() {
        return candidates.to_vec();
    }

    // Pre-compute lowercased needles once.
    let needles: Vec<String> = terms
        .iter()
        .map(|t| extract_search_term(&t.text).to_lowercase())
        .collect();
    let needle_refs: Vec<&str> = needles.iter().map(|s| s.as_str()).collect();

    let mut out = Vec::with_capacity(candidates.len());

    for &fid in candidates {
        // Fast path: try filename first (no path reconstruction).
        let name = index.get_file_name(fid);
        if path_contains_all_terms(name, &needle_refs) {
            out.push(fid);
            continue;
        }

        // Slow path: reconstruct full path only if needed.
        let path = index.reconstruct_full_path(fid);
        if path_contains_all_terms(&path, &needle_refs) {
            out.push(fid);
        }
    }

    out
}

/// Check whether *all* needles appear (case-insensitive) in the given path.
#[inline]
fn path_contains_all_terms(path: &str, needles: &[&str]) -> bool {
    for &needle in needles {
        if !contains_lowercase_ascii(path, needle) {
            return false;
        }
    }
    true
}

/// Core implementation of text search against the base index.
fn eval_text_base_with_state<I: IndexReader>(
    index: &I,
    state: &TextSearchState,
    candidates: &[FileId],
) -> Vec<FileId> {
    if candidates.is_empty() {
        return Vec::new();
    }

    // Very short needles or tiny candidate sets: just scan.
    if !state.is_trigram_capable() || candidates.len() <= SMALL_CANDIDATE_CUTOFF {
        return eval_text_linear_scan_with_paths(index, &state.needle_lower, candidates);
    }

    let file_count = index.get_file_count();
    if file_count == 0 {
        return Vec::new();
    }

    // Choose informative trigrams, ordered by rarity.
    let threshold = (file_count as f64 * MAX_TRIGRAM_GLOBAL_SHARE) as usize;
    let mut items: SmallVec<[(Trigram, usize); 8]> = SmallVec::new();

    for &tri in &state.trigrams {
        let len = index.trigram_postings_len(tri);

        if len == 0 {
            // Missing trigram => no file path contains the full needle.
            return Vec::new();
        }

        if len <= threshold {
            items.push((tri, len));
        }
    }

    if items.is_empty() {
        // All trigrams are too broad; trigram seeding doesn't help.
        return eval_text_linear_scan_with_paths(index, &state.needle_lower, candidates);
    }

    items.sort_unstable_by_key(|&(_, len)| len);
    items.truncate(MAX_TRIGRAMS_PER_QUERY);

    let effective_tris: SmallVec<[Trigram; 8]> = items.into_iter().map(|(t, _)| t).collect();

    // Intersect candidate set with trigram postings.
    let tri_candidates = get_file_trigram_candidates(index, &effective_tris, candidates);

    if tri_candidates.is_empty() {
        return Vec::new();
    }

    // Full verification via substring matching on full path.
    let mut out = Vec::with_capacity(tri_candidates.len());

    for &fid in &tri_candidates {
        // Try filenames first so as to avoid path reconstruction for many cases.
        let name = index.get_file_name(fid);
        if contains_lowercase_ascii(name, &state.needle_lower) {
            out.push(fid);
            continue;
        }

        // If filename doesn't match, check the full path
        let path = index.reconstruct_full_path(fid);
        if contains_lowercase_ascii(&path, &state.needle_lower) {
            out.push(fid);
        }
    }

    out
}

/// Fallback path for short terms or when trigram filtering is not useful.
///
/// `needle_lower` must already be lowercased.
fn eval_text_linear_scan_with_paths<I: IndexReader>(
    index: &I,
    needle_lower: &str,
    candidates: &[FileId],
) -> Vec<FileId> {
    if needle_lower.is_empty() {
        return candidates.to_vec();
    }

    let mut out = Vec::new();
    out.reserve(candidates.len());

    for &fid in candidates {
        // Fast path: filename first.
        let name = index.get_file_name(fid);
        if contains_lowercase_ascii(name, needle_lower) {
            out.push(fid);
            continue;
        }

        // Slow path: full path includes directories.
        let path = index.reconstruct_full_path(fid);
        if contains_lowercase_ascii(&path, needle_lower) {
            out.push(fid);
        }
    }

    out
}

/// Intersect global trigram postings with the current candidate set.
///
/// Both `candidates` and postings are assumed sorted ascending.
fn get_file_trigram_candidates<I: IndexReader>(
    index: &I,
    trigrams: &[Trigram],
    candidates: &[FileId],
) -> Vec<FileId> {
    if trigrams.is_empty() || candidates.is_empty() {
        return Vec::new();
    }

    // Sort trigrams by postings length (rarest first).
    let mut tris: SmallVec<[(Trigram, usize); 8]> = SmallVec::new();
    tris.extend(trigrams.iter().copied().map(|t| {
        let len = index.trigram_postings_len(t);
        (t, len)
    }));
    tris.sort_unstable_by_key(|&(_, len)| len);

    let mut buf_a: Vec<FileId> = Vec::new();
    let mut buf_b: Vec<FileId> = Vec::new();
    let mut current_is_a = true;
    let mut has_current = false;

    for (tri, _) in tris {
        let postings = match index.query_trigram(tri) {
            Some(v) => v,
            None => return Vec::new(),
        };

        if !has_current {
            // First intersection: postings ∩ candidates
            buf_a = intersect_adaptive(candidates, postings);
            if buf_a.is_empty() {
                return Vec::new();
            }
            if buf_a.len() <= EARLY_VERIFY_CUTOFF {
                return buf_a;
            }
            has_current = true;
            current_is_a = true;
            continue;
        }

        if current_is_a {
            intersect_adaptive_into(buf_a.as_slice(), postings, &mut buf_b);
            if buf_b.is_empty() {
                return Vec::new();
            }
            if buf_b.len() <= EARLY_VERIFY_CUTOFF {
                return buf_b;
            }
            current_is_a = false;
        } else {
            intersect_adaptive_into(buf_b.as_slice(), postings, &mut buf_a);
            if buf_a.is_empty() {
                return Vec::new();
            }
            if buf_a.len() <= EARLY_VERIFY_CUTOFF {
                return buf_a;
            }
            current_is_a = true;
        }
    }

    if !has_current {
        Vec::new()
    } else if current_is_a {
        buf_a
    } else {
        buf_b
    }
}
