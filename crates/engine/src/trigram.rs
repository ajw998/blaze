use smallvec::SmallVec;

const INLINE: usize = 32;

/// Pack trigrams as a 4-byte integer.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Trigram(u32);

impl Trigram {
    #[inline]
    pub const fn from_bytes(b0: u8, b1: u8, b2: u8) -> Self {
        let v = (b0 as u32) | ((b1 as u32) << 8) | ((b2 as u32) << 16);
        Trigram(v)
    }

    #[inline]
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    /// Caller is responsible for ensuring the upper 8 bits are unused if
    /// they care about roundtripping via `to_bytes`.
    #[inline]
    pub const fn from_u32(v: u32) -> Self {
        Trigram(v)
    }

    #[inline]
    pub const fn to_bytes(self) -> [u8; 3] {
        let v = self.0;
        [
            (v & 0xFF) as u8,
            ((v >> 8) & 0xFF) as u8,
            ((v >> 16) & 0xFF) as u8,
        ]
    }
}

/// ASCII-only case folding for a single byte.
///
/// - 'A'..'Z' = 'a'..'z'
/// - All other bytes (including non-ASCII) are returned unchanged.
#[inline]
fn ascii_fold(b: u8) -> u8 {
    if b.is_ascii_uppercase() { b + 32 } else { b }
}

/// Normalize arbitrary bytes (e.g. Unix path bytes) for trigram indexing.
///
/// - ASCII letters are lowercased.
/// - Non-ASCII bytes (including UTF-8 for Chinese, etc.) are left unchanged.
/// - Output is stored in a `SmallVec` to keep short paths on the stack.
#[inline]
pub fn normalize_for_trigram_bytes(input: &[u8]) -> SmallVec<[u8; INLINE]> {
    let mut out: SmallVec<[u8; INLINE]> = SmallVec::with_capacity(input.len());
    out.extend(input.iter().map(|&b| ascii_fold(b)));
    out
}

/// Convenience helper for UTF-8 text queries.
#[inline]
pub fn normalize_for_trigram_str(s: &str) -> SmallVec<[u8; INLINE]> {
    normalize_for_trigram_bytes(s.as_bytes())
}

/// Build a sorted, deduplicated set of trigrams from normalized bytes.
///
/// `normalized` is assumed to already be case-normalized via
/// `normalize_for_trigram_bytes` or `normalize_for_trigram_str`.
///
/// If `normalized.len() < 3`, this returns an empty vector. The caller must
/// treat such strings as "not indexable by trigrams" and fall back to a
/// linear scan or another strategy. There is no efficient, correct trigram
/// filter for queries shorter than 3 bytes without sentinels or more
/// complex machinery.
fn build_trigrams_from_normalized(normalized: &[u8]) -> Vec<Trigram> {
    if normalized.len() < 3 {
        return Vec::new();
    }

    let mut tris = Vec::with_capacity(normalized.len().saturating_sub(2));
    for win in normalized.windows(3) {
        tris.push(Trigram::from_bytes(win[0], win[1], win[2]));
    }

    tris.sort_unstable();
    tris.dedup();
    tris
}

/// Build a sorted, deduplicated set of trigrams for a UTF-8 string.
///
/// This is primarily useful for tests or non-path data; for actual filesystem
/// paths prefer `build_trigrams_for_bytes` to avoid UTF-8 assumptions.
pub fn build_trigrams_for_string(s: &str) -> Vec<Trigram> {
    let normalized = normalize_for_trigram_str(s);
    build_trigrams_from_normalized(&normalized)
}

/// Build a sorted, deduplicated set of trigrams for arbitrary bytes
/// (e.g. Unix paths, possibly non-UTF-8).
pub fn build_trigrams_for_bytes(bytes: &[u8]) -> Vec<Trigram> {
    let normalized = normalize_for_trigram_bytes(bytes);
    build_trigrams_from_normalized(&normalized)
}

/// Build trigrams for a query term using non-overlapping selection.
///
/// This is used for query execution (not indexing) and provides a performance
/// optimization by generating fewer trigrams while maintaining good selectivity.
///
/// # Difference from full trigram set
///
/// - **Indexing** (`build_trigrams_for_bytes` / `build_trigrams_for_string`):
///   uses a sliding window over all bytes (no sentinels).
///   - Example: "commands" → "com", "omm", "mma", "man", "and", "nds"
///
/// - **Querying** (this function): uses mostly non-overlapping trigrams.
///   - Example: "commands" → "com", "man", "nds"
///
/// Because we always take a subset of the query's true trigrams, the filter
/// is *weaker* (more candidates) but never loses true matches: any text that
/// contains the query substring must contain *all* of its trigrams, and
/// therefore also the subset we select here.
///
/// # Short queries
///
/// For `len(text) < 3` there are no trigrams at all. In that case we return an
/// empty vector; the caller must fall back to a linear scan or another index.
/// Attempting to use trigrams for such queries either degenerates into
/// "union of almost everything" or becomes incorrect.
///
/// # Performance impact
///
/// Typical reduction in trigram count for queries:
/// - "config" (6 bytes): 4 trigrams → 2 trigrams
/// - "commands" (8 bytes): 6 trigrams → 3 trigrams
/// - "lib_controller" (14 bytes): 12 trigrams → 5 trigrams
///
/// Since each trigram implies an index lookup + intersection, this directly
/// translates into faster query execution.
pub fn build_query_trigrams(text: &str) -> Vec<Trigram> {
    let bytes = normalize_for_trigram_str(text);

    if bytes.len() < 3 {
        return Vec::new();
    }

    let mut tris: Vec<Trigram> = Vec::new();
    let mut i = 0;

    // Generate trigrams at 3-byte intervals.
    // This gives us a subset of all trigrams with good information content.
    while i + 3 <= bytes.len() {
        let tri = Trigram::from_bytes(bytes[i], bytes[i + 1], bytes[i + 2]);
        tris.push(tri);
        i += 3;
    }

    // Add the last trigram if we didn't cover the end.
    // This ensures suffix coverage, e.g. "abcdefgh" → [abc, def, fgh].
    if bytes.len() > 3 && bytes.len() % 3 != 0 {
        let last_pos = bytes.len() - 3;
        let last_tri =
            Trigram::from_bytes(bytes[last_pos], bytes[last_pos + 1], bytes[last_pos + 2]);
        if tris.last() != Some(&last_tri) {
            tris.push(last_tri);
        }
    }

    tris.sort_unstable();
    tris.dedup();
    tris
}

#[cfg(test)]
#[path = "trigram_tests.rs"]
mod tests;
