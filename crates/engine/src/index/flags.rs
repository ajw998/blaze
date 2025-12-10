use bitflags::bitflags;
use blaze_fs::FileRecord;
use blaze_runtime::{CACHE_COMPONENTS, NOISY_COMPONENTS, SYSTEM_ROOTS};

const VERY_DEEP_THRESHOLD: usize = 15;

bitflags! {
    /// File flags. These are flags defined for in-memory metadata instead of the raw OS mode bits.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct FileFlags: u16 {
        // Structural
        const IS_DIR = 0b0000_0000_00000_0001;
        /// Whether the file is a symlink.
        const IS_SYMLINK = 0b0000_0000_0000_0010;
        /// Special files include socket, fifo, device etc.
        /// These are generally non-regular filesystem nodes we do not
        /// want showing up in our search results.
        const SPECIAL = 0b0000_0000_0000_0100;
        /// Visibility and exclusions
        const HIDDEN = 0b0000_0000_0000_1000;
        /// Ignore patterns
        const EXCLUDED_GLOB = 0b0000_0000_0001_0000;
        /// Whether the user explicitly hid the file
        const EXCLUDED_USER = 0b0000_0000_0010_0000;
        /// Whether the particular file is in the "Trash".
        const IN_TRASH = 0b00000_0000_100_0000;
    }
}

bitflags! {
    /// Noise classification flags for ranking purposes.
    ///
    /// These flags identify paths that are typically less relevant to users,
    /// allowing the ranking system to demote them while still keeping them
    /// searchable for specific queries.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct NoiseFlags: u8 {
        /// System directories: /usr, /lib, /var, /etc, /opt, /proc, /sys
        const SYSTEM_DIR   = 0b0000_0001;
        /// Build/dependency directories
        const BUILD_DIR    = 0b0000_0010;
        /// Cache directories
        const CACHE_DIR    = 0b0000_0100;
        /// Path segment looks like a hash/UUID
        const HASHY_SEG    = 0b0000_1000;
        /// Very deep path (>15 levels)
        const VERY_DEEP    = 0b0001_0000;
        /// Application data directories
        const APP_DATA_DIR = 0b0010_0000;
        /// Log/debug directories: debug, logs, sessionstore-logs
        const LOG_DIR      = 0b0100_0000;
    }
}

/// Classify a path's noise characteristics.
///
/// Returns (NoiseFlags, path_depth) computed from the path string.
/// This is designed to be called at index time to avoid per-query overhead.
///
/// # Note
/// Paths are assumed to be valid UTF-8. Non-UTF-8 paths should be handled
/// by the caller (e.g., using `to_string_lossy()`).
pub fn classify_noise(path: &str) -> (NoiseFlags, u8) {
    let mut flags = NoiseFlags::empty();

    // System roots check - case-insensitive on macOS, exact on Linux
    if is_system_path(path) {
        flags |= NoiseFlags::SYSTEM_DIR;
    }

    // Single-pass through components to minimize allocations
    let mut depth = 0usize;
    let mut has_build = false;
    let mut has_cache = false;
    let mut has_hash = false;
    let mut has_log = false;
    // Track hidden directory depth: if we see a .something directory,
    // count how many levels deep we go after it
    let mut in_hidden_app_dir = false;
    let mut depth_after_hidden = 0usize;

    for comp in path.split('/').filter(|s| !s.is_empty()) {
        depth += 1;

        // Track depth after entering a hidden directory
        if in_hidden_app_dir {
            depth_after_hidden += 1;
        }

        // Detect hidden directories (start with . but not . or ..)
        // Also detect .local/share pattern
        if !in_hidden_app_dir && is_hidden_app_component(comp) {
            in_hidden_app_dir = true;
        }

        if !has_build && is_noisy_component(comp) {
            has_build = true;
        }
        if !has_cache && is_cache_component(comp) {
            has_cache = true;
        }
        if !has_log && is_log_component(comp) {
            has_log = true;
        }
        if !has_hash && is_hashy(comp) {
            has_hash = true;
        }
    }

    if has_build {
        flags |= NoiseFlags::BUILD_DIR;
    }
    if has_cache {
        flags |= NoiseFlags::CACHE_DIR;
    }
    if has_hash {
        flags |= NoiseFlags::HASHY_SEG;
    }
    if has_log {
        flags |= NoiseFlags::LOG_DIR;
    }
    // Only flag as APP_DATA if we went 2+ levels deep into a hidden directory
    // This avoids penalizing ~/.bashrc but does penalize ~/.mozilla/firefox/profile/...
    if in_hidden_app_dir && depth_after_hidden >= 2 {
        flags |= NoiseFlags::APP_DATA_DIR;
    }

    let depth_u8 = depth.min(255) as u8;
    match depth > VERY_DEEP_THRESHOLD {
        true => {
            flags |= NoiseFlags::VERY_DEEP;
        }
        false => (),
    }

    (flags, depth_u8)
}

/// Check if path is under a system root directory
#[inline]
fn is_system_path(path: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        // macOS: case-insensitive filesystem
        let path_lower = path.to_ascii_lowercase();
        SYSTEM_ROOTS.iter().any(|root| path_lower.starts_with(root))
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Linux and others: case-sensitive filesystem
        SYSTEM_ROOTS.iter().any(|root| path.starts_with(root))
    }
}

/// Check if component matches a noisy (build/dependency) directory
#[inline]
fn is_noisy_component(comp: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        let comp_lower = comp.to_ascii_lowercase();
        NOISY_COMPONENTS.iter().any(|n| *n == comp_lower)
    }
    #[cfg(not(target_os = "macos"))]
    {
        NOISY_COMPONENTS.contains(&comp)
    }
}

/// Check if component matches a cache directory
#[inline]
fn is_cache_component(comp: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        let comp_lower = comp.to_ascii_lowercase();
        CACHE_COMPONENTS.iter().any(|n| *n == comp_lower)
    }
    #[cfg(not(target_os = "macos"))]
    {
        CACHE_COMPONENTS.contains(&comp)
    }
}

/// Check if component matches a log/debug directory
#[inline]
fn is_log_component(comp: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        use blaze_runtime::LOG_COMPONENTS;

        let comp_lower = comp.to_ascii_lowercase();
        LOG_COMPONENTS.iter().any(|n| *n == comp_lower)
    }
    #[cfg(not(target_os = "macos"))]
    {
        use blaze_runtime::LOG_COMPONENTS;

        LOG_COMPONENTS.contains(&comp)
    }
}

/// Check if component is a hidden directory that likely contains app data.
///
/// Returns true for directories starting with `.` (excluding `.` and `..`),
/// as well as `.local` (XDG data directory pattern).
#[inline]
fn is_hidden_app_component(comp: &str) -> bool {
    if !comp.starts_with('.') {
        return false;
    }
    // Exclude . and ..
    if comp == "." || comp == ".." {
        return false;
    }
    true
}

/// Detect if a path segment looks like a hash or generated identifier.
///
/// Distinguishes hashes like `c7653396db1f627dc568685e0043c4f8` or UUIDs like
/// `6186feed-abb5-4bb6-b116-f0178b81fa0f` from human-readable names.
///
/// Criteria for pure hashes:
/// - Length between 16 and 64 characters (common hash lengths)
/// - High proportion of hex characters (>85%)
/// - No word separators (underscores, dots)
///
/// Criteria for UUIDs:
/// - Exactly 36 characters (8-4-4-4-12 with hyphens)
/// - Hyphens at positions 8, 13, 18, 23
/// - All other characters are hex
#[inline]
fn is_hashy(s: &str) -> bool {
    let len = s.len();

    // Check for UUID format first (36 chars: 8-4-4-4-12)
    if len == 36 && is_uuid_format(s) {
        return true;
    }

    // Common hash lengths: 16 (half MD5), 32 (MD5), 40 (SHA1), 64 (SHA256)
    if !(16..=64).contains(&len) {
        return false;
    }

    // Human-readable names typically have separators; files have extensions
    // Using bytes for performance (all separators are ASCII)
    let bytes = s.as_bytes();
    if bytes.contains(&b'_') || bytes.contains(&b'-') || bytes.contains(&b'.') {
        return false;
    }

    // Count hex characters using bytes (avoids UTF-8 decoding overhead)
    let hex_count = bytes.iter().filter(|b| b.is_ascii_hexdigit()).count();
    let hex_ratio = hex_count as f32 / len as f32;

    // High hex ratio indicates hash-like content
    hex_ratio > 0.85
}

/// Check if string matches UUID format: 8-4-4-4-12 (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx)
#[inline]
fn is_uuid_format(s: &str) -> bool {
    let bytes = s.as_bytes();

    // Check hyphens at correct positions
    if bytes[8] != b'-' || bytes[13] != b'-' || bytes[18] != b'-' || bytes[23] != b'-' {
        return false;
    }

    // Check all other characters are hex
    bytes
        .iter()
        .enumerate()
        .all(|(i, &b)| matches!(i, 8 | 13 | 18 | 23) || b.is_ascii_hexdigit())
}

/// Compute the noise penalty score from NoiseFlags and depth.
///
/// Returns a positive penalty value (higher = more noisy, less relevant).
///
/// # Penalty weights (tunable)
/// - HASHY_SEG (60): Generated identifiers like git commit hashes, UUIDs
/// - SYSTEM_DIR (50): OS directories rarely contain user files
/// - APP_DATA_DIR (45): Application-generated data (.mozilla/firefox/...)
/// - BUILD_DIR (40): Build artifacts (node_modules, target, dist)
/// - LOG_DIR (35): Log and debug directories
/// - CACHE_DIR (30): Caches occasionally have relevant config files
/// - VERY_DEEP (20): Deep nesting is a mild signal
/// - Depth penalty: +2 per level beyond depth 8 (max +40)
pub fn compute_noise_penalty(flags: NoiseFlags, depth: u8) -> i32 {
    let mut penalty = 0i32;

    if flags.contains(NoiseFlags::HASHY_SEG) {
        penalty += 60;
    }
    if flags.contains(NoiseFlags::SYSTEM_DIR) {
        penalty += 50;
    }
    if flags.contains(NoiseFlags::APP_DATA_DIR) {
        penalty += 45; // Browser profiles, tool state, etc.
    }
    if flags.contains(NoiseFlags::BUILD_DIR) {
        penalty += 40; // Build artifacts are noisier than caches
    }
    if flags.contains(NoiseFlags::LOG_DIR) {
        penalty += 35; // Log and debug directories
    }
    if flags.contains(NoiseFlags::CACHE_DIR) {
        penalty += 30; // Caches may have user-relevant config
    }
    if flags.contains(NoiseFlags::VERY_DEEP) {
        penalty += 20; // Mild signal
    }

    // Additional depth-based penalty beyond threshold
    let extra_depth = depth.saturating_sub(8) as i32;
    penalty += extra_depth.min(20) * 2;

    penalty
}

impl FileFlags {
    pub fn default_search_exclude() -> FileFlags {
        FileFlags::HIDDEN
            | FileFlags::EXCLUDED_GLOB
            | FileFlags::EXCLUDED_USER
            | FileFlags::IN_TRASH
            | FileFlags::SPECIAL
    }

    #[inline]
    pub fn is_default_visible(self) -> bool {
        !self.intersects(Self::default_search_exclude())
    }
}

pub fn compute_file_flags(
    input: &FileRecord,
    excluded_glob: bool,
    excluded_user: bool,
) -> FileFlags {
    let mut flags = FileFlags::empty();

    if input.is_dir {
        flags.insert(FileFlags::IS_DIR);
    }
    if input.is_symlink {
        flags.insert(FileFlags::IS_SYMLINK);
    }
    if input.is_special {
        flags.insert(FileFlags::SPECIAL);
    }
    if input.hidden_os {
        flags.insert(FileFlags::HIDDEN);
    }
    if input.in_trash {
        flags.insert(FileFlags::IN_TRASH);
    }
    if excluded_glob {
        flags.insert(FileFlags::EXCLUDED_GLOB);
    }
    if excluded_user {
        flags.insert(FileFlags::EXCLUDED_USER);
    }

    flags
}
