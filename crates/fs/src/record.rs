use std::path::PathBuf;

#[derive(Debug)]
pub struct FileRecord {
    pub full_path: PathBuf,
    /// File name
    pub name: String,
    /// File size
    pub size: u64,
    /// File last modified time
    pub mtime_secs: u64,
    /// File creation time
    pub ctime_secs: u64,
    /// File last accessed time (may be unavailable on some platforms/mount options)
    pub atime_secs: u64,
    /// Lowercase extension without dot e.g., 'pdf'
    pub ext: Option<String>,
    /// Visibility and exclusions
    pub is_dir: bool,
    pub is_symlink: bool,
    pub is_special: bool,
    pub in_trash: bool,
    pub ignored_glob: bool,
    pub hidden_os: bool,
    pub user_excludes: bool,
}
