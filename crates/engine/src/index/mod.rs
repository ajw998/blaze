use std::{
    fs::File,
    io::{self, Error, ErrorKind},
    mem,
    path::Path,
    str,
};

use bytemuck::{Pod, Zeroable, cast_slice, from_bytes};
use memmap2::{Mmap, MmapOptions};

use crate::{Trigram, helpers::blob_str};

pub mod builder;
pub mod compat;
pub mod flags;
pub mod helpers;
pub mod persist;
pub mod reader;

pub use builder::*;
pub use persist::*;
pub use reader::*;

pub type FileId = u32;
pub type DirId = u32;
pub type ExtId = u16;

pub struct Index {
    mmap: Mmap,
    header: IndexHeader,
    ext_table: Vec<String>,
    file_metas_offset: usize,
    file_metas_len_bytes: usize,
    dirs_offset: usize,
    dirs_len_bytes: usize,
    names_blob_offset: usize,
    names_blob_len: usize,

    ext_index_keys_offset: usize,
    ext_index_keys_len: usize,
    ext_index_postings_offset: usize,
    ext_index_postings_len: usize,

    trigram_keys_offset: usize,
    trigram_keys_len: usize,
    trigram_postings_offset: usize,
    trigram_postings_len: usize,

    dir_trigram_keys_offset: usize,
    dir_trigram_keys_len: usize,
    dir_trigram_postings_offset: usize,
    dir_trigram_postings_len: usize,
}

/// Describes a section within the index file.
/// All offsets are absolute byte offsets from file start.
/// Sections containing aligned types (u32, u64) must start at 8-byte boundaries.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SectionDesc {
    /// Absolute byte offset from start of file
    pub offset: u64,
    /// Length in bytes
    pub len: u64,
    /// Section flags (bit 0 = compressed, others reserved)
    pub flags: u32,
    /// Reserved for future use
    pub _reserved: u32,
}

impl SectionDesc {
    /// Section contains LZ4-compressed data
    pub const FLAG_COMPRESSED: u32 = 1 << 0;
    /// Section contains delta-encoded integers
    pub const FLAG_DELTA_ENCODED: u32 = 1 << 1;

    /// Create a new section descriptor with no flags
    #[inline]
    pub fn new(offset: u64, len: u64) -> Self {
        Self {
            offset,
            len,
            flags: 0,
            _reserved: 0,
        }
    }

    /// Check if section is compressed
    #[inline]
    pub fn is_compressed(&self) -> bool {
        self.flags & Self::FLAG_COMPRESSED != 0
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct IndexHeader {
    /// Magic number (INDEX_MAGIC)
    pub magic: u32,
    /// Index format version
    pub version: u32,
    /// Size of this header in bytes (for forward compatibility)
    pub header_size: u32,
    /// CRC32 of header bytes [0..header_size), with this field set to 0
    pub header_crc32: u32,
    /// Bitflags describing how this index was built
    pub flags_bits: u32,
    /// Number of files indexed
    pub file_count: u32,
    /// Number of directories indexed
    pub dir_count: u32,
    /// Number of distinct extensions
    pub ext_count: u32,
    // Reserved (16 bytes)
    pub reserved: [u8; 16],
    // Section descriptors
    /// Index metadata
    pub metadata: SectionDesc,
    /// Encoded file extension table
    pub ext_table: SectionDesc,
    pub dirs: SectionDesc,
    pub files_meta: SectionDesc,
    pub names_blob: SectionDesc,

    pub ext_index_keys: SectionDesc,
    pub ext_index_postings: SectionDesc,

    pub trigram_keys: SectionDesc,
    pub trigram_postings: SectionDesc,

    pub dir_trigram_keys: SectionDesc,
    pub dir_trigram_postings: SectionDesc,
}

// Disk Structs

/// Build metadata stored in the index_meta section.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct IndexMeta {
    /// Unix timestamp when index was created
    pub created_secs: u64,
    /// Offset into names_blob for the root path
    pub root_path_offset: u32,
    /// Length of root path in names_blob
    pub root_path_len: u32,
    /// Build flags (follow_symlinks, etc.)
    pub build_flags: u32,
    /// Reserved
    pub _reserved: u32,
}

bitflags::bitflags! {
    #[repr(transparent)]
    pub struct FileFlagsBits: u16 {
    }
}

bitflags::bitflags! {
    #[repr(transparent)]
    pub struct NoiseFlagsBits: u16 {
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct FileMeta {
    pub size: u64,
    /// File last modified time (u32 is valid until year 2106)
    pub mtime_secs: u32,
    /// File creation time (u32 is valid until year 2106)
    pub ctime_secs: u32,
    /// File last accessed time (may be 0 if unavailable)
    pub atime_secs: u32,
    pub dir_id: u32,
    /// Offset in the index
    pub name_offset: u32,
    /// Offset length
    pub name_len: u32,
    /// File extension code mapped to index's extension table
    pub ext_id: u16,
    /// File flags (e.g., directory, hidden, symlink)
    pub flag_bits: u16,
    /// Noise classification flags for ranking
    pub noise_bits: u8,
    /// Path depth (number of components)
    pub path_depth: u8,
    /// Padding for 8-byte alignment (struct contains u64, so must be 8-byte aligned)
    pub _reserved: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct DirMeta {
    pub name_offset: u32,
    pub name_len: u32,
    // u32::MAX for no parents (root)
    pub parent: u32,
    pub flags_bits: u16,
    pub _reserved: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ExtKey {
    /// Extension id (index into ext_table)
    pub ext_id: u16,
    /// Padding to keep 4-byte alignment for the following fields
    pub _pad: u16,
    /// Offset into the ext_postings array
    pub postings_offset: u32,
    /// Number of FileIds in this posting list
    pub postings_len: u32,
    /// Reserved for future use
    pub _reserved: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct TrigramKey {
    // 3 bytes packed + 1 padding byte
    pub trigram: u32,
    pub postings_offset: u32,
    // Number of FileIds
    pub postings_len: u32,
    /// Reserved for future use
    pub _reserved: u32,
}

/// The on-disk, mmap'd Index.
/// Provides zero-copy access to the Index.
/// Do NOT use this to build an index. There is a dedicated builder for that.
/// See [IndexBuilder]
impl Index {
    pub fn open(path: &Path) -> io::Result<Self> {
        let (mmap, header) = map_and_read_header(path)?;
        verify_index_header(&mmap, &header)?;
        let ext_table = decode_ext_table(&mmap, &header)?;
        Ok(Self::from_mmap(mmap, header, ext_table))
    }

    fn from_mmap(mmap: Mmap, header: IndexHeader, ext_table: Vec<String>) -> Self {
        Self {
            mmap,
            header,
            ext_table,
            file_metas_offset: header.files_meta.offset as usize,
            file_metas_len_bytes: header.files_meta.len as usize,
            dirs_offset: header.dirs.offset as usize,
            dirs_len_bytes: header.dirs.len as usize,
            names_blob_offset: header.names_blob.offset as usize,
            names_blob_len: header.names_blob.len as usize,
            ext_index_keys_offset: header.ext_index_keys.offset as usize,
            ext_index_keys_len: header.ext_index_keys.len as usize,
            ext_index_postings_offset: header.ext_index_postings.offset as usize,
            ext_index_postings_len: header.ext_index_postings.len as usize,
            trigram_keys_offset: header.trigram_keys.offset as usize,
            trigram_keys_len: header.trigram_keys.len as usize,
            trigram_postings_offset: header.trigram_postings.offset as usize,
            trigram_postings_len: header.trigram_postings.len as usize,
            dir_trigram_keys_offset: header.dir_trigram_keys.offset as usize,
            dir_trigram_keys_len: header.dir_trigram_keys.len as usize,
            dir_trigram_postings_offset: header.dir_trigram_postings.offset as usize,
            dir_trigram_postings_len: header.dir_trigram_postings.len as usize,
        }
    }

    #[inline]
    fn file_metas(&self) -> &[FileMeta] {
        let start = self.file_metas_offset;
        let end = start + self.file_metas_len_bytes;
        cast_slice(&self.mmap[start..end])
    }

    #[inline]
    fn dirs(&self) -> &[DirMeta] {
        let start = self.dirs_offset;
        let end = start + self.dirs_len_bytes;
        cast_slice(&self.mmap[start..end])
    }

    #[inline]
    fn names_blob(&self) -> &[u8] {
        &self.mmap[self.names_blob_offset..self.names_blob_offset + self.names_blob_len]
    }

    #[inline]
    fn trigram_keys(&self) -> &[TrigramKey] {
        let start = self.trigram_keys_offset;
        let end = start + self.trigram_keys_len;
        cast_slice(&self.mmap[start..end])
    }

    #[inline]
    fn trigram_postings_raw(&self) -> &[u32] {
        let start = self.trigram_postings_offset;
        let end = start + self.trigram_postings_len;
        cast_slice(&self.mmap[start..end])
    }

    #[inline]
    fn dir_trigram_keys(&self) -> &[TrigramKey] {
        let start = self.dir_trigram_keys_offset;
        let end = start + self.dir_trigram_keys_len;
        cast_slice(&self.mmap[start..end])
    }

    #[inline]
    fn dir_trigram_postings_raw(&self) -> &[u32] {
        let start = self.dir_trigram_postings_offset;
        let end = start + self.dir_trigram_postings_len;
        cast_slice(&self.mmap[start..end])
    }

    #[inline]
    fn trigram_postings_slice(&self, key: &TrigramKey) -> Option<&[u32]> {
        let postings = self.trigram_postings_raw();

        let start = key.postings_offset as usize;
        let end = start + key.postings_len as usize;

        if end > postings.len() {
            return None;
        }

        Some(&postings[start..end])
    }

    #[inline]
    fn ext_keys(&self) -> &[ExtKey] {
        let start = self.ext_index_keys_offset;
        let end = start + self.ext_index_keys_len;
        cast_slice(&self.mmap[start..end])
    }

    #[inline]
    fn ext_postings_raw(&self) -> &[u32] {
        let start = self.ext_index_postings_offset;
        let end = start + self.ext_index_postings_len;
        cast_slice(&self.mmap[start..end])
    }

    #[inline]
    pub fn ext_postings(&self, ext_id: ExtId) -> &[FileId] {
        let keys = self.ext_keys();
        let idx = ext_id as usize;
        if idx >= keys.len() {
            return &[];
        }
        let key = &keys[idx];
        debug_assert_eq!(key.ext_id, ext_id);

        let postings = self.ext_postings_raw();
        let start = key.postings_offset as usize;
        let end = start + key.postings_len as usize;
        if end > postings.len() {
            return &[];
        }

        &postings[start..end]
    }

    /// Zero-copy file trigram lookup.
    #[inline]
    pub fn query_trigram_on_disk(&self, tri: Trigram) -> Option<&[u32]> {
        let keys = self.trigram_keys();
        let target = tri.as_u32();

        let idx = keys.binary_search_by_key(&target, |k| k.trigram).ok()?;
        let key = &keys[idx];

        self.trigram_postings_slice(key)
    }

    /// Zero-copy *directory* trigram lookup.
    #[inline]
    pub fn query_dir_trigram_on_disk(&self, tri: Trigram) -> Option<&[u32]> {
        let keys = self.dir_trigram_keys();
        let postings = self.dir_trigram_postings_raw();

        let target = tri.as_u32();

        let idx = keys.binary_search_by_key(&target, |k| k.trigram).ok()?;
        let key = &keys[idx];

        let start = key.postings_offset as usize;
        let end = start + key.postings_len as usize;

        if end > postings.len() {
            return None;
        }

        Some(&postings[start..end])
    }
    #[inline]
    pub fn get_name(&self, offset: u32, len: u32) -> &str {
        let blob = self.names_blob();
        blob_str(blob, offset, len)
    }

    pub fn root_path(&self) -> Option<&str> {
        let meta = self.read_index_meta()?;
        Some(self.get_name(meta.root_path_offset, meta.root_path_len))
    }

    fn read_index_meta(&self) -> Option<&IndexMeta> {
        let desc = self.header.metadata;
        if desc.len < mem::size_of::<IndexMeta>() as u64 {
            return None;
        }
        let start = desc.offset as usize;
        let end = start + mem::size_of::<IndexMeta>();
        Some(from_bytes(&self.mmap[start..end]))
    }

    pub fn reconstruct_relative_path(&self, file_id: FileId) -> String {
        let metas = self.file_metas();
        let dirs = self.dirs();

        let meta = &metas[file_id as usize];
        let mut components: Vec<&str> = Vec::with_capacity(meta.path_depth as usize + 1);

        // file name
        components.push(self.get_name(meta.name_offset, meta.name_len));

        // dir chain
        let mut d = meta.dir_id;
        loop {
            if d == u32::MAX {
                break;
            }
            let dir = &dirs[d as usize];
            let name = self.get_name(dir.name_offset, dir.name_len);
            if !name.is_empty() {
                components.push(name);
            }
            if dir.parent == u32::MAX {
                break;
            }
            d = dir.parent;
        }

        components.reverse();
        components.join("/")
    }

    pub fn reconstruct_absolute_path(&self, file_id: FileId) -> Option<String> {
        let root = self.root_path()?;
        let rel = self.reconstruct_relative_path(file_id);
        let mut s = String::with_capacity(root.len() + 1 + rel.len());
        s.push_str(root);
        if !root.ends_with('/') {
            s.push('/');
        }
        s.push_str(&rel);
        Some(s)
    }
}

fn map_and_read_header(path: &Path) -> io::Result<(Mmap, IndexHeader)> {
    let file = File::open(path)?;
    let mmap = unsafe { MmapOptions::new().map(&file)? };

    let file_len = mmap.len();
    let header_size = mem::size_of::<IndexHeader>();

    if file_len < header_size {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "index file too small for header",
        ));
    }

    let header_bytes = &mmap[..header_size];
    let header: IndexHeader = *from_bytes(header_bytes);

    Ok((mmap, header))
}

fn decode_ext_table(mmap: &Mmap, header: &IndexHeader) -> io::Result<Vec<String>> {
    let ext_off = header.ext_table.offset as usize;
    let ext_end = ext_off + header.ext_table.len as usize;
    let ext_bytes = &mmap[ext_off..ext_end];

    let mut exts = Vec::new();

    // Simple NUL-separated decode
    for part in ext_bytes.split(|b| *b == 0) {
        if part.is_empty() {
            exts.push(String::new());
        } else {
            exts.push(String::from_utf8_lossy(part).into_owned());
        }
    }

    Ok(exts)
}

fn verify_index_header(mmap: &Mmap, header: &IndexHeader) -> io::Result<()> {
    let file_len = mmap.len();
    let header_size = mem::size_of::<IndexHeader>();

    // Basic bound check: header must fit
    if file_len < header_size {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "index file too small for header",
        ));
    }

    if header.magic != INDEX_MAGIC {
        return Err(Error::new(ErrorKind::InvalidData, "invalid index magic"));
    }

    if header.version != INDEX_VERSION {
        return Err(Error::new(ErrorKind::InvalidData, "index version mismatch"));
    }

    for section in [
        header.metadata,
        header.ext_table,
        header.dirs,
        header.files_meta,
        header.names_blob,
        header.ext_index_keys,
        header.ext_index_postings,
        header.trigram_keys,
        header.trigram_postings,
        header.dir_trigram_keys,
        header.dir_trigram_postings,
    ] {
        let start = section.offset as usize;
        let len = section.len as usize;
        let end = start
            .checked_add(len)
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "section length overflow"))?;

        if end > file_len {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "section lies outside index file",
            ));
        }

        // TODO: alignment checks for sections
    }

    // TODO: header CRC32 check
    // compute_crc32(&mmap[..header.header_size as usize], with header_crc32 field zeroed)

    Ok(())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
