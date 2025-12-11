use std::path::PathBuf;

pub const PROGRAM_NAME: &str = "blaze";
pub const PROGRAM_LOG_LEVEL: &str = "BLAZE_LOG_LEVEL";
// TODO - Change this to be dynamically generated
pub const INDEX_FILE_NAME: &str = "index.bin";

pub fn xdg_or_home(xdg_var: &str, home_suffix: &str) -> PathBuf {
    if let Some(dir) = std::env::var_os(xdg_var) {
        PathBuf::from(dir)
    } else {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(home_suffix)
    }
}

/// Default root for the program to start scanning
pub fn default_scan_root() -> PathBuf {
    // Try to get the user's home directory using environment variables
    #[cfg(unix)]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                // Fallback to current directory if HOME is not set
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
            })
    }
    #[cfg(windows)]
    {
        // On Windows, try USERPROFILE first, then HOMEDRIVE+HOMEPATH
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| {
                let drive = std::env::var_os("HOMEDRIVE")?;
                let path = std::env::var_os("HOMEPATH")?;
                Some(PathBuf::from(drive).join(path))
            })
            .unwrap_or_else(|| {
                // Fallback to current directory
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
            })
    }
    #[cfg(not(any(unix, windows)))]
    {
        // For other platforms, use current directory
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }
}

pub fn blaze_dir() -> PathBuf {
    xdg_or_home("XDG_CACHE_HOME", ".cache").join(PROGRAM_NAME)
}

/// Default index file path
pub fn default_index_path() -> PathBuf {
    blaze_dir().join(INDEX_FILE_NAME)
}

/// Default project-relative ignore patterns for common build artifacts, VCS dirs, etc.
pub const DEFAULT_PROJECT_IGNORE_PATTERNS: &[&str] = &[
    "venv/",
    ".venv/",
    "build/",
    ".cache/",
    "dist/",
    ".DS_Store",
    ".git/",
    ".hg/",
    ".svn/",
    "node_modules/",
    "target/",
    "Thumbs.db",
    "vendor/",
    "lost+found/",
];

/// System-ish directories to skip when the scan root is `/`.
pub const DEFAULT_SYSTEM_SKIP_PREFIXES: &[&str] = &[
    "/proc",
    "/sys",
    "/dev",
    "/run",
    "/var/run",
    "/var/tmp",
    "/private/tmp",
];

/// System root directories that are typically not user-relevant
/// Entries are lowercase for case-insensitive matching on macOS
#[cfg(target_os = "linux")]
pub const SYSTEM_ROOTS: &[&str] = &[
    "/usr/",
    "/lib/",
    "/lib64/",
    "/opt/",
    "/snap/",
    "/flatpak/",
    "/var/",
    "/etc/",
    "/sys/",
    "/proc/",
];

#[cfg(target_os = "macos")]
const SYSTEM_ROOTS: &[&str] = &[
    "/system/",
    "/library/",
    "/applications/",
    "/private/var/",
    "/private/etc/",
    "/cores/",
    "/usr/",
    "/opt/",
    "/var/",
];

// Fallback for other platforms
#[cfg(not(any(target_os = "linux", target_os = "macos")))]

const SYSTEM_ROOTS: &[&str] = &["/usr/", "/lib/", "/opt/", "/var/", "/etc/"];
/// Build, dependency, and VCS directories
/// All entries must be lowercase for case-insensitive matching on macOS
pub const NOISY_COMPONENTS: &[&str] = &[
    "node_modules",
    "target",
    "build",
    "dist",
    "out",
    ".next",
    ".git",
    ".hg",
    ".svn",
    ".venv",
    "venv",
    "site-packages",
    ".tox",
    "vendor",
    ".cargo",
];

/// Cache-specific directories (matched as path components)
/// All entries must be lowercase for case-insensitive matching on macOS
pub const CACHE_COMPONENTS: &[&str] = &[
    ".cache",
    "cache",
    ".gradle",
    ".m2",
    ".npm",
    ".pip",
    "caches",
    "__pycache__",
];

/// Log and debug directories
pub const LOG_COMPONENTS: &[&str] = &[
    "logs",
    "log",
    "debug",
    "sessionstore-logs",
    "crash-reports",
    "crashreporter",
    "telemetry",
    "diagnostics",
];
