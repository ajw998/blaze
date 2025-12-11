use std::{
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use tempfile::NamedTempFile;

use bytemuck::{bytes_of, cast_slice};
use crc32fast::Hasher;

use crate::{
    ExtKey,
    index::{DirMeta, FileMeta, IndexHeader, IndexMeta, SectionDesc, StagedIndex, TrigramKey},
};

/// Alignment for sections containing structs with u64/u32 fields.
/// Kept consistent with the rest of the index layout.
const SECTION_ALIGNMENT: u64 = 8;
/// Magic number: "BLZE" in little-endian
pub const INDEX_MAGIC: u32 = 0x455A4C42;

pub const INDEX_VERSION: u32 = 1;

/// Align `value` up to the next multiple of `alignment`
#[inline]
fn align_up(value: u64, alignment: u64) -> u64 {
    debug_assert!(alignment.is_power_of_two());
    (value + alignment - 1) & !(alignment - 1)
}

/// Encode extension table as a simple '\0'-separated list of UTF-8 strings.
/// First entry is the reserved "" for "no extension".
fn encode_ext_table(exts: &[String]) -> Vec<u8> {
    // ext_table is tiny, no need for elaborate encoding.
    let mut buf = Vec::new();
    for ext in exts {
        buf.extend_from_slice(ext.as_bytes());
        // Add NUL terminator
        buf.push(0);
    }
    buf
}

/// Encode directory metadata array
fn encode_dirs(dirs: &[DirMeta]) -> Vec<u8> {
    cast_slice(dirs).to_vec()
}

/// Encode file metadata array
fn encode_file_metas(files: &[FileMeta]) -> Vec<u8> {
    cast_slice(files).to_vec()
}

/// Encode a slice of u32 IDs (e.g. sorted FileIds) as raw bytes.
fn encode_u32_slice(ids: &[u32]) -> Vec<u8> {
    cast_slice(ids).to_vec()
}

fn encode_ext_keys(keys: &[ExtKey]) -> Vec<u8> {
    cast_slice(keys).to_vec()
}

/// Encode trigram keys (Pod, repr(C)).
fn encode_trigram_keys(keys: &[TrigramKey]) -> Vec<u8> {
    cast_slice(keys).to_vec()
}

/// Write a `StagedIndex` to an open file positioned at start.
///
/// `flags_bits` is the raw bitmask
pub fn write_index_to(file: &File, index: &StagedIndex, index_flags: u32) -> io::Result<()> {
    let mut writer = BufWriter::new(file);

    let created_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let index_meta = IndexMeta {
        created_secs,
        root_path_offset: index.root_path_offset,
        root_path_len: index.root_path_len,
        // TODO: Currently no build-time options. We might just add them later
        build_flags: 0,
        _reserved: 0,
    };
    let index_meta_bytes = bytes_of(&index_meta);

    // Sections
    let ext_table_bytes = encode_ext_table(&index.ext_table);
    let dirs_bytes = encode_dirs(&index.dirs);
    let file_metas_bytes = encode_file_metas(&index.files);

    // names_blob is already final (includes root path at root_path_offset)
    let names_blob_bytes = &index.names_blob;

    let ext_index_keys_bytes = encode_ext_keys(&index.ext_index_keys);
    let ext_index_postings_bytes = encode_u32_slice(&index.ext_index_postings);

    let trigram_keys_bytes = encode_trigram_keys(&index.file_trigram_keys);
    let trigram_postings_bytes = encode_u32_slice(&index.file_trigram_postings);

    let dir_trigram_keys_bytes = encode_trigram_keys(&index.dir_trigram_keys);
    let dir_trigram_postings_bytes = encode_u32_slice(&index.dir_trigram_postings);

    // Computes section offset
    let header_size = std::mem::size_of::<IndexHeader>() as u64;
    let mut offset = header_size;

    // metadata (IndexMeta): aligned
    offset = align_up(offset, SECTION_ALIGNMENT);
    let metadata_section = SectionDesc::new(offset, index_meta_bytes.len() as u64);
    offset += metadata_section.len;

    // ext_table: raw bytes, no extra alignment
    let ext_table_section = SectionDesc::new(offset, ext_table_bytes.len() as u64);
    offset += ext_table_section.len;

    // dirs: contains u32, align
    offset = align_up(offset, SECTION_ALIGNMENT);
    let dirs_section = SectionDesc::new(offset, dirs_bytes.len() as u64);
    offset += dirs_section.len;

    // files_meta: contains u64, align
    offset = align_up(offset, SECTION_ALIGNMENT);
    let files_meta_section = SectionDesc::new(offset, file_metas_bytes.len() as u64);
    offset += files_meta_section.len;

    // names_blob: plain bytes
    let names_blob_section = SectionDesc::new(offset, names_blob_bytes.len() as u64);
    offset += names_blob_section.len;

    // ext index keys
    offset = align_up(offset, SECTION_ALIGNMENT);
    let ext_index_keys_section = SectionDesc::new(offset, ext_index_keys_bytes.len() as u64);
    offset += ext_index_keys_section.len;

    // ext index postings
    offset = align_up(offset, SECTION_ALIGNMENT);
    let ext_index_postings_section =
        SectionDesc::new(offset, ext_index_postings_bytes.len() as u64);
    offset += ext_index_postings_section.len;

    // file trigram keys: contains u32, align
    offset = align_up(offset, SECTION_ALIGNMENT);
    let trigram_keys_section = SectionDesc::new(offset, trigram_keys_bytes.len() as u64);
    offset += trigram_keys_section.len;

    // file trigram postings: u32 array, align
    offset = align_up(offset, SECTION_ALIGNMENT);
    let trigram_postings_section = SectionDesc::new(offset, trigram_postings_bytes.len() as u64);
    offset += trigram_postings_section.len;

    // dir trigram keys: contains u32, align
    offset = align_up(offset, SECTION_ALIGNMENT);
    let dir_trigram_keys_section = SectionDesc::new(offset, dir_trigram_keys_bytes.len() as u64);
    offset += dir_trigram_keys_section.len;

    // dir trigram postings: u32 array, align
    offset = align_up(offset, SECTION_ALIGNMENT);
    let dir_trigram_postings_section =
        SectionDesc::new(offset, dir_trigram_postings_bytes.len() as u64);
    let _final_end = dir_trigram_postings_section.offset + dir_trigram_postings_section.len;

    // Header (CRC32 over header bytes with crc field zeroed)
    let mut header = IndexHeader {
        magic: INDEX_MAGIC,
        version: INDEX_VERSION,
        header_size: header_size as u32,
        header_crc32: 0,
        flags_bits: index_flags,
        file_count: index.files.len() as u32,
        dir_count: index.dirs.len() as u32,
        ext_count: index.ext_table.len() as u32,
        reserved: [0u8; 16],
        metadata: metadata_section,
        ext_table: ext_table_section,
        dirs: dirs_section,
        files_meta: files_meta_section,
        names_blob: names_blob_section,
        ext_index_keys: ext_index_keys_section,
        ext_index_postings: ext_index_postings_section,
        trigram_keys: trigram_keys_section,
        trigram_postings: trigram_postings_section,
        dir_trigram_keys: dir_trigram_keys_section,
        dir_trigram_postings: dir_trigram_postings_section,
    };

    let mut hasher = Hasher::new();
    hasher.update(bytes_of(&header));
    header.header_crc32 = hasher.finalize();

    const ZERO_PAD: [u8; SECTION_ALIGNMENT as usize] = [0u8; SECTION_ALIGNMENT as usize];

    #[inline]
    fn write_padding<W: Write>(writer: &mut W, current: u64, target: u64) -> io::Result<()> {
        debug_assert!(target >= current);
        let padding = (target - current) as usize;
        if padding > 0 {
            writer.write_all(&ZERO_PAD[..padding])?;
        }
        Ok(())
    }

    let mut pos = 0u64;

    // Header
    writer.write_all(bytes_of(&header))?;
    pos += header_size;

    // metadata
    write_padding(&mut writer, pos, metadata_section.offset)?;
    pos = metadata_section.offset;
    writer.write_all(index_meta_bytes)?;
    pos += metadata_section.len;

    // ext_table
    writer.write_all(&ext_table_bytes)?;
    pos += ext_table_bytes.len() as u64;

    // Dirs
    write_padding(&mut writer, pos, dirs_section.offset)?;
    pos = dirs_section.offset;
    writer.write_all(&dirs_bytes)?;
    pos += dirs_section.len;

    // File metas
    write_padding(&mut writer, pos, files_meta_section.offset)?;
    pos = files_meta_section.offset;
    writer.write_all(&file_metas_bytes)?;
    pos += files_meta_section.len;

    // names_blob (no alignment)
    writer.write_all(names_blob_bytes)?;
    pos += names_blob_bytes.len() as u64;

    // extension keys
    write_padding(&mut writer, pos, ext_index_keys_section.offset)?;
    pos = ext_index_keys_section.offset;
    writer.write_all(&ext_index_keys_bytes)?;
    pos += ext_index_keys_section.len;

    // extension postings
    write_padding(&mut writer, pos, ext_index_postings_section.offset)?;
    pos = ext_index_postings_section.offset;
    writer.write_all(&ext_index_postings_bytes)?;
    pos += ext_index_postings_section.len;

    // file trigram keys
    write_padding(&mut writer, pos, trigram_keys_section.offset)?;
    pos = trigram_keys_section.offset;
    writer.write_all(&trigram_keys_bytes)?;
    pos += trigram_keys_section.len;

    // file trigram postings
    write_padding(&mut writer, pos, trigram_postings_section.offset)?;
    pos = trigram_postings_section.offset;
    writer.write_all(&trigram_postings_bytes)?;
    pos += trigram_postings_section.len;

    // dir trigram keys
    write_padding(&mut writer, pos, dir_trigram_keys_section.offset)?;
    pos = dir_trigram_keys_section.offset;
    writer.write_all(&dir_trigram_keys_bytes)?;
    pos += dir_trigram_keys_section.len;

    // dir trigram postings
    write_padding(&mut writer, pos, dir_trigram_postings_section.offset)?;
    writer.write_all(&dir_trigram_postings_bytes)?;

    writer.flush()?;
    Ok(())
}

/// Atomic index write
pub fn write_index_atomic(path: &Path, index: &StagedIndex, flags_bits: u32) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let tmp = NamedTempFile::new_in(parent)?;

    write_index_to(tmp.as_file(), index, flags_bits)?;

    tmp.as_file().sync_all()?;

    // Atomically rename temp file to target path
    tmp.persist(path).map_err(|e| e.error)?;

    #[cfg(unix)]
    {
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
    }

    Ok(())
}
