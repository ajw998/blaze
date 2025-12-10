use std::path::{Path, PathBuf};

use blaze_fs::FileRecord;
use hashbrown::{HashMap, hash_map::Entry};

use crate::{
    DirId, ExtId, ExtKey, FileId,
    index::{
        DirMeta, FileMeta, TrigramKey,
        flags::{FileFlags, classify_noise, compute_file_flags},
    },
    trigram::{Trigram, build_trigrams_for_bytes},
};

pub struct StagedIndex {
    pub root: PathBuf,
    pub names_blob: Vec<u8>,
    pub root_path_offset: u32,
    pub root_path_len: u32,
    pub dirs: Vec<DirMeta>,
    pub files: Vec<FileMeta>,
    pub ext_table: Vec<String>,

    pub ext_index_keys: Vec<ExtKey>,
    pub ext_index_postings: Vec<u32>,

    pub file_trigram_keys: Vec<TrigramKey>,
    pub file_trigram_postings: Vec<u32>,

    pub dir_trigram_keys: Vec<TrigramKey>,
    pub dir_trigram_postings: Vec<u32>,
}

/// IndexBuilder is responsible for ingesting FileRecords
/// from our fs walker, which produces [FileRecord].
#[derive(Debug)]
pub struct IndexBuilder {
    root: PathBuf,
    names_blob: Vec<u8>,
    dirs: Vec<DirMeta>,
    dir_map: HashMap<PathBuf, DirId>,
    files: Vec<FileMeta>,
    ext_table: Vec<String>,
    ext_map: HashMap<String, ExtId>,
    ext_postings: Vec<Vec<FileId>>,
    file_trigrams: HashMap<Trigram, Vec<FileId>>,
    dir_trigrams: HashMap<Trigram, Vec<DirId>>,
    root_path_offset: u32,
    root_path_len: u32,
}

/// Narrow u64 timestamp to u32 for on-disk storage.
fn narrow_time(t: u64) -> u32 {
    if t > u32::MAX as u64 {
        u32::MAX
    } else {
        t as u32
    }
}

fn intern_string(buf: &mut Vec<u8>, s: &str) -> (u32, u32) {
    let offset = buf.len() as u32;
    buf.extend_from_slice(s.as_bytes());
    let len = s.len() as u32;
    (offset, len)
}

fn pack_trigram_map(map: HashMap<Trigram, Vec<u32>>) -> (Vec<TrigramKey>, Vec<u32>) {
    let mut entries: Vec<(Trigram, Vec<u32>)> = map.into_iter().collect();

    // We must ensure that all trigrams are sorted
    entries.sort_by_key(|(tri, _)| tri.as_u32());

    // Pre-compute capacities to avoid reallocs
    let total_postings: usize = entries.iter().map(|(_, v)| v.len()).sum();
    let mut keys = Vec::with_capacity(entries.len());
    let mut postings = Vec::with_capacity(total_postings);

    let mut offset: u32 = 0;
    for (tri, mut v) in entries {
        v.sort_unstable(); // in-place

        let len = v.len() as u32;
        postings.extend_from_slice(&v);

        keys.push(TrigramKey {
            trigram: tri.as_u32(),
            postings_offset: offset,
            postings_len: len,
            _reserved: 0,
        });

        offset += len;
    }

    (keys, postings)
}

fn pack_ext_postings(ext_postings: Vec<Vec<FileId>>) -> (Vec<ExtKey>, Vec<u32>) {
    let mut keys = Vec::with_capacity(ext_postings.len());
    let total_postings: usize = ext_postings.iter().map(|v| v.len()).sum();
    let mut postings = Vec::with_capacity(total_postings);

    let mut offset: u32 = 0;
    for (ext_id, v) in ext_postings.into_iter().enumerate() {
        // v is already sorted by FileId (we append in monotonically increasing file_id order)
        let len = v.len() as u32;
        postings.extend_from_slice(&v);

        keys.push(ExtKey {
            ext_id: ext_id as ExtId,
            _pad: 0,
            postings_offset: offset,
            postings_len: len,
            _reserved: 0,
        });

        offset += len;
    }

    (keys, postings)
}

/// Build trigrams for a filesystem path.
///
/// On Unix we index raw path bytes (no UTF-8 assumptions). On other
/// platforms we fall back to a UTF-8 lossy string representation.
#[cfg(unix)]
fn path_trigrams(path: &Path) -> Vec<Trigram> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    build_trigrams_for_bytes(bytes)
}

#[cfg(not(unix))]
fn path_trigrams(path: &Path) -> Vec<Trigram> {
    // Fallback: no direct access to raw bytes, so we rely on UTF-8.
    let s = path.to_string_lossy();
    build_trigrams_for_string(&s)
}

// TODO: Move this out
pub struct BuildResult {
    /// Number of files indexed
    pub file_count: usize,
    /// Warning messages
    pub warning: Option<String>,
}

impl IndexBuilder {
    pub fn new(root: PathBuf) -> Self {
        let mut names_blob = Vec::with_capacity(1024);

        // Intern root path string up front
        let root_str = root.to_string_lossy();
        let (root_path_offset, root_path_len) = intern_string(&mut names_blob, &root_str);

        // ext_table[0] reserved for "no extension"
        let ext_table = vec![];

        let mut ext_postings = Vec::new();
        ext_postings.push(Vec::new());

        Self {
            root,
            names_blob,
            dirs: Vec::new(),
            dir_map: HashMap::new(),
            files: Vec::new(),
            ext_postings,
            ext_table,
            ext_map: HashMap::new(),
            file_trigrams: HashMap::new(),
            dir_trigrams: HashMap::new(),
            root_path_offset,
            root_path_len,
        }
    }

    pub fn add_batch<I>(&mut self, batch: I)
    where
        I: IntoIterator<Item = FileRecord>,
    {
        for rec in batch {
            self.add_record(rec);
        }
    }

    pub fn add_record(&mut self, record: FileRecord) {
        let name = &record.name;
        let (name_offset, name_len) = intern_string(&mut self.names_blob, name);

        let full_path = &record.full_path;

        let mtime_secs = narrow_time(record.mtime_secs);
        let ctime_secs = narrow_time(record.ctime_secs);
        let atime_secs = narrow_time(record.atime_secs);
        let file_id = self.files.len() as FileId;

        let rel = match full_path.strip_prefix(&self.root) {
            Ok(p) => p,
            // fallback to absolute path
            Err(_) => full_path.as_path(),
        };

        let rel_dir = rel.parent().unwrap_or_else(|| Path::new(""));

        let ext_id = self.intern_ext(record.ext.as_deref());

        let dir_id = self.get_or_insert_dir(rel_dir);

        self.ext_postings[ext_id as usize].push(file_id);

        let path_str = full_path.to_string_lossy();

        let (noise_flags, path_depth) = classify_noise(&path_str);

        let file_flags = compute_file_flags(&record, record.ignored_glob, record.user_excludes);

        self.files.push(FileMeta {
            atime_secs,
            ctime_secs,
            dir_id,
            ext_id,
            flag_bits: file_flags.bits(),
            mtime_secs,
            name_len,
            name_offset,
            noise_bits: noise_flags.bits(),
            path_depth,
            size: record.size,
            _reserved: 0,
        });

        // Build trigram index for files and dirs (relative path only).
        self.add_trigrams(file_id, &record, rel, dir_id, file_flags);
    }

    /// Get or create a DirId for a *relative* directory path.
    fn get_or_insert_dir(&mut self, rel_dir: &Path) -> DirId {
        // If it is an empty path, it is a root relative directory or file,
        // in which case return u32::MAX
        if rel_dir.as_os_str().is_empty() {
            return u32::MAX;
        }

        if let Some(&id) = self.dir_map.get(rel_dir) {
            return id;
        }

        // Ensure parent exists
        let parent_id = match rel_dir.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => self.get_or_insert_dir(parent),
            _ => u32::MAX,
        };

        // Directory name is the last component
        let name = rel_dir
            .file_name()
            .map(|os| os.to_string_lossy().into_owned())
            .unwrap_or_else(String::new);

        let (name_offset, name_len) = intern_string(&mut self.names_blob, &name);

        let id = self.dirs.len() as DirId;
        self.dirs.push(DirMeta {
            name_offset,
            name_len,
            parent: parent_id,
            flags_bits: 0,
            _reserved: 0,
        });

        self.dir_map.insert(rel_dir.to_path_buf(), id);
        id
    }

    pub fn intern_ext(&mut self, ext: Option<&str>) -> ExtId {
        match ext {
            None => 0,
            Some(e) => match self.ext_map.entry(e.to_string()) {
                Entry::Occupied(o) => *o.get(),
                Entry::Vacant(v) => {
                    let id = self.ext_table.len() as ExtId;
                    self.ext_table.push(e.to_string());
                    self.ext_postings.push(Vec::new());
                    v.insert(id);
                    id
                }
            },
        }
    }

    fn add_trigrams(
        &mut self,
        file_id: FileId,
        rec: &FileRecord,
        rel: &Path,
        dir_id: DirId,
        flags: FileFlags,
    ) {
        if rec.is_dir {
            // Directory trigram index: relative directory path only.
            let trigrams = path_trigrams(rel);
            for tri in trigrams {
                self.dir_trigrams.entry(tri).or_default().push(dir_id);
            }
            return;
        }

        // Skip invisible files in the trigram index.
        if !flags.is_default_visible() {
            return;
        }

        // File trigram index: relative file path only.
        let trigrams = path_trigrams(rel);
        for tri in trigrams {
            self.file_trigrams.entry(tri).or_default().push(file_id);
        }
    }

    pub fn finish(self) -> StagedIndex {
        let (file_trigram_keys, file_trigram_postings) = pack_trigram_map(self.file_trigrams);
        let (dir_trigram_keys, dir_trigram_postings) = pack_trigram_map(self.dir_trigrams);
        let (ext_index_keys, ext_index_postings) = pack_ext_postings(self.ext_postings);

        StagedIndex {
            root: self.root,
            names_blob: self.names_blob,
            root_path_offset: self.root_path_offset,
            root_path_len: self.root_path_len,
            dirs: self.dirs,
            files: self.files,
            ext_table: self.ext_table,
            ext_index_keys,
            ext_index_postings,
            file_trigram_keys,
            file_trigram_postings,
            dir_trigram_keys,
            dir_trigram_postings,
        }
    }
}
