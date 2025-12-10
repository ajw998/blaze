use blaze_runtime::DEFAULT_PROJECT_IGNORE_PATTERNS;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Path, PathBuf};

pub struct IgnoreEngine {
    matcher: Gitignore,
}

#[derive(Default)]
pub struct TrashConfig {
    pub(crate) roots: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct UserExcludes {
    roots: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct IgnoreOptions {
    /// Whether to use the default ignore patterns
    pub use_default_patterns: bool,

    /// Paths to additional ignore files
    pub extra_ignore_files: Box<[PathBuf]>,
}

impl Default for IgnoreEngine {
    fn default() -> Self {
        // Empty matcher rooted at the current directory; callers can opt into
        // project patterns via `with_defaults`/`new` when a root is available.
        let matcher = GitignoreBuilder::new(Path::new("."))
            .build()
            .expect("build empty ignore matcher");
        IgnoreEngine { matcher }
    }
}

impl TrashConfig {
    pub fn new() -> Self {
        let mut roots = Vec::new();

        // Platform-specific defaults
        #[cfg(target_os = "macos")]
        {
            if let Some(home) = dirs::home_dir() {
                roots.push(home.join(".Trash"));
            }
            // On macOS, each volume can have a .Trashes directory at the root.
            // We don't know all mount points here, but we can at least add the
            // global one.
            roots.push(PathBuf::from("/.Trashes"));
        }

        #[cfg(target_os = "linux")]
        {
            // $XDG_DATA_HOME/Trash, or ~/.local/share/Trash
            if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
                roots.push(PathBuf::from(xdg).join("Trash"));
            } else if let Some(home) = dirs::home_dir() {
                roots.push(home.join(".local/share/Trash"));
            }
        }

        #[cfg(target_os = "windows")]
        {
            // Windows Recycle Bin is per-volume under "$Recycle.Bin".
            // We approximate by treating any "$Recycle.Bin" path as trash.
            // (volume roots are like "C:\" etc.)
            // We don't enumerate all drives here, we just match by name prefix.
            // The check below will treat any path containing "$Recycle.Bin" as trash.
        }

        let roots = roots
            .into_iter()
            .filter_map(|p| p.canonicalize().ok().or(Some(p)))
            .collect();

        TrashConfig { roots }
    }

    #[inline]
    pub fn is_in_trash(&self, path: &Path) -> bool {
        if self.roots.iter().any(|root| path.starts_with(root)) {
            return true;
        }

        // Windows approximate detection: any path containing "$Recycle.Bin"
        #[cfg(target_os = "windows")]
        {
            if path
                .components()
                .any(|c| c.as_os_str().eq_ignore_ascii_case("$Recycle.Bin"))
            {
                return true;
            }
        }

        false
    }

    pub fn add_root(&mut self, root: PathBuf) {
        let root = root.canonicalize().unwrap_or(root);
        self.roots.push(root);
    }
}

impl Default for IgnoreOptions {
    fn default() -> Self {
        Self {
            use_default_patterns: true,
            extra_ignore_files: Box::default(),
        }
    }
}

impl IgnoreEngine {
    /// Build an IgnoreEngine rooted at `root`, with default *project* patterns and optional extra ignore files.
    pub fn new(root: &Path, options: Option<IgnoreOptions>) -> Result<Self, ignore::Error> {
        let IgnoreOptions {
            use_default_patterns,
            extra_ignore_files,
        } = options.unwrap_or_default();
        let mut builder = GitignoreBuilder::new(root);

        if use_default_patterns {
            for pat in DEFAULT_PROJECT_IGNORE_PATTERNS {
                builder.add_line(None, pat)?;
            }
        }

        for path in &*extra_ignore_files {
            builder.add(path);
        }

        Ok(IgnoreEngine {
            matcher: builder.build()?,
        })
    }

    #[inline]
    pub fn with_defaults(root: &Path) -> Result<Self, ignore::Error> {
        Self::new(root, None)
    }

    #[inline]
    #[must_use]
    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        self.matcher
            .matched_path_or_any_parents(path, is_dir)
            .is_ignore()
    }
}

impl UserExcludes {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        UserExcludes { roots }
    }

    pub fn with_system_defaults() -> Self {
        let mut ux = UserExcludes::new(Vec::new());
        #[cfg(unix)]
        {
            use blaze_runtime::DEFAULT_SYSTEM_SKIP_PREFIXES;

            for p in DEFAULT_SYSTEM_SKIP_PREFIXES {
                ux.add_root(PathBuf::from(p));
            }
        }
        ux
    }

    pub fn add_root(&mut self, root: PathBuf) {
        // Canonicalize here because on certain systems, /var/run
        // actually points to /run.
        let root = match root.canonicalize() {
            Ok(p) => p,
            Err(_) => root,
        };

        // Check if this root is already covered by an existing root
        // If true, then new root is a child of existing root
        for existing in &self.roots {
            if root.starts_with(existing) {
                return;
            }
        }

        // Remove any existing roots that are children of the new root
        self.roots.retain(|existing| !existing.starts_with(&root));

        self.roots.push(root);
    }

    #[inline]
    pub fn is_excluded(&self, path: &Path) -> bool {
        self.roots.iter().any(|root| path.starts_with(root))
    }
}

impl Default for UserExcludes {
    fn default() -> Self {
        UserExcludes::with_system_defaults()
    }
}

#[cfg(test)]
#[path = "excludes_tests.rs"]
mod tests;
