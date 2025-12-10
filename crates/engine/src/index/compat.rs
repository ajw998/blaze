// crates/engine/src/index/compat.rs

use std::{
    fs::File,
    io::{self, ErrorKind, Read, Seek, SeekFrom},
    mem,
    path::{Path, PathBuf},
};

use bytemuck::from_bytes;

use super::{INDEX_MAGIC, INDEX_VERSION, IndexHeader, IndexMeta};

pub enum IndexCompatibility {
    Missing,
    Corrupt,
    VersionMismatch { on_disk: u32, expected: u32 },
    RootMismatch { on_disk: PathBuf, expected: PathBuf },
    Ok(Box<IndexHeader>),
}

/// Check index header compatibility (existence, magic, version, flags).
///
/// This is a *cheap* probe:
/// - Only reads the fixed-size header from disk
/// - Returns `Ok(Missing/Corrupt/…/Ok(..))` for logical outcomes
/// - Only returns `Err(io::Error)` for actual OS/I/O failures (e.g. open denied)
pub fn check_index_header(path: &Path) -> io::Result<IndexCompatibility> {
    if !path.exists() {
        return Ok(IndexCompatibility::Missing);
    }

    let mut file = File::open(path)?;

    let mut buf = [0u8; mem::size_of::<IndexHeader>()];
    if let Err(e) = file.read_exact(&mut buf) {
        eprintln!("[index] failed to read header from {path:?}: {e}");
        return Ok(IndexCompatibility::Corrupt);
    }

    // SAFETY: IndexHeader is Pod, buffer is exactly the right size.
    let header_ref: &IndexHeader = from_bytes(&buf);
    let header: IndexHeader = *header_ref;

    // Basic sanity: magic
    if header.magic != INDEX_MAGIC {
        return Ok(IndexCompatibility::Corrupt);
    }

    // Version check
    if header.version != INDEX_VERSION {
        return Ok(IndexCompatibility::VersionMismatch {
            on_disk: header.version,
            expected: INDEX_VERSION,
        });
    }

    Ok(IndexCompatibility::Ok(Box::new(header)))
}

/// Read the stored root path from the index without constructing a full `Index`
fn read_index_root(path: &Path, header: &IndexHeader) -> io::Result<PathBuf> {
    let mut file = File::open(path)?;

    let meta_desc = header.metadata;
    let meta_size = mem::size_of::<IndexMeta>() as u64;

    if meta_desc.len < meta_size {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "index metadata section too small for IndexMeta",
        ));
    }

    file.seek(SeekFrom::Start(meta_desc.offset))?;

    let mut meta_buf = [0u8; mem::size_of::<IndexMeta>()];
    file.read_exact(&mut meta_buf)?;
    let meta_ref: &IndexMeta = from_bytes(&meta_buf);
    let meta: IndexMeta = *meta_ref;

    let names_desc = header.names_blob;

    let root_off = meta.root_path_offset as u64;
    let root_len = meta.root_path_len as u64;

    if root_off.checked_add(root_len).unwrap_or(u64::MAX) > names_desc.len {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "root path lies outside names_blob section",
        ));
    }

    let abs_root_start = names_desc.offset + root_off;
    let root_len_usize = meta.root_path_len as usize;
    let mut root_buf = vec![0u8; root_len_usize];

    file.seek(SeekFrom::Start(abs_root_start))?;
    file.read_exact(&mut root_buf)?;

    let root_str = String::from_utf8_lossy(&root_buf).into_owned();
    Ok(PathBuf::from(root_str))
}

/// Check full index compatibility including root-path validation.
///
/// - Delegates to `check_index_header` for header/magic/version/flags.
/// - If header is OK, also verifies that the stored root path matches the
///   requested root (after canonicalisation).
pub fn check_index_compatibility(
    path: &Path,
    requested_root: &Path,
) -> io::Result<IndexCompatibility> {
    match check_index_header(path)? {
        IndexCompatibility::Ok(header) => {
            match read_index_root(path, &header) {
                Ok(on_disk_root) => {
                    // Canonicalise the requested root; if that fails, fall back.
                    let canonical_requested = requested_root
                        .canonicalize()
                        .unwrap_or_else(|_| requested_root.to_path_buf());

                    // Canonicalise the stored root as well; if it no longer exists,
                    // compare the raw stored path.
                    let canonical_on_disk =
                        on_disk_root.canonicalize().unwrap_or(on_disk_root.clone());

                    if canonical_on_disk != canonical_requested {
                        Ok(IndexCompatibility::RootMismatch {
                            on_disk: canonical_on_disk,
                            expected: canonical_requested,
                        })
                    } else {
                        Ok(IndexCompatibility::Ok(header))
                    }
                }
                Err(_) => {
                    // Could not read metadata/root → treat as Corrupt rather than I/O error,
                    // so callers see a logical compatibility result.
                    Ok(IndexCompatibility::Corrupt)
                }
            }
        }
        other => Ok(other),
    }
}
