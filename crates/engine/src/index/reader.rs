use crate::{
    index::{DirId, FileId, Index, flags::NoiseFlags},
    trigram::Trigram,
};

pub trait IndexReader {
    /// Get number of indexed files
    fn get_file_count(&self) -> usize;
    /// Directory count
    fn dir_count(&self) -> usize;
    /// Get the filename
    fn get_file_name(&self, id: FileId) -> &str;
    fn get_file_dir_id(&self, id: FileId) -> u32;
    fn get_dir_name(&self, id: DirId) -> &str;
    /// Get file extension
    /// Returns lowercase extension, empty string if None
    fn get_file_ext(&self, id: FileId) -> &str;
    /// Get file size
    fn get_file_size(&self, id: FileId) -> u64;
    /// Get the modified time as seconds since Unix epoch
    fn get_file_modified_epoch(&self, id: FileId) -> i64;
    /// Get the created time as seconds since Unix epoch
    fn get_file_created_epoch(&self, id: FileId) -> i64;
    /// Get the noise classification flags.
    fn get_file_noise_bits(&self, id: FileId) -> NoiseFlags;
    /// Get the noise classification flags.
    fn get_file_path_depth(&self, id: FileId) -> u8;
    /// Query a trigram slice
    fn query_trigram(&self, tri: Trigram) -> Option<&[u32]>;
    /// Query Directory Trigram
    fn query_dir_trigram(&self, tri: Trigram) -> Option<&[u32]>;

    #[inline]
    fn trigram_postings_len(&self, tri: Trigram) -> usize {
        self.query_trigram(tri).map_or(0, |p| p.len())
    }

    fn reconstruct_full_path(&self, id: FileId) -> String;
}

impl IndexReader for Index {
    fn get_file_count(&self) -> usize {
        self.header.file_count as usize
    }

    fn dir_count(&self) -> usize {
        self.header.dir_count as usize
    }

    fn get_dir_name(&self, id: DirId) -> &str {
        let dirs = self.dirs();
        if let Some(dir) = dirs.get(id as usize) {
            self.get_name(dir.name_offset, dir.name_len)
        } else {
            ""
        }
    }

    fn get_file_name(&self, id: FileId) -> &str {
        let metas = self.file_metas();
        if let Some(meta) = metas.get(id as usize) {
            self.get_name(meta.name_offset, meta.name_len)
        } else {
            ""
        }
    }

    fn get_file_dir_id(&self, id: FileId) -> DirId {
        self.file_metas()
            .get(id as usize)
            .map(|m| m.dir_id)
            .unwrap_or(u32::MAX)
    }

    fn get_file_ext(&self, id: FileId) -> &str {
        let metas = self.file_metas();
        if let Some(meta) = metas.get(id as usize) {
            if meta.ext_id == 0 {
                ""
            } else {
                self.ext_table
                    .get(meta.ext_id as usize)
                    .map(|s| s.as_str())
                    .unwrap_or("")
            }
        } else {
            ""
        }
    }

    fn get_file_size(&self, id: FileId) -> u64 {
        self.file_metas()
            .get(id as usize)
            .map(|m| m.size)
            .unwrap_or(0)
    }

    fn get_file_modified_epoch(&self, id: FileId) -> i64 {
        self.file_metas()
            .get(id as usize)
            .map(|m| m.mtime_secs as i64)
            .unwrap_or(0)
    }

    fn get_file_created_epoch(&self, id: FileId) -> i64 {
        self.file_metas()
            .get(id as usize)
            .map(|m| m.ctime_secs as i64)
            .unwrap_or(0)
    }

    fn get_file_noise_bits(&self, id: FileId) -> NoiseFlags {
        self.file_metas()
            .get(id as usize)
            .map(|m| NoiseFlags::from_bits_truncate(m.noise_bits))
            .unwrap_or(NoiseFlags::empty())
    }

    fn get_file_path_depth(&self, id: FileId) -> u8 {
        self.file_metas()
            .get(id as usize)
            .map(|m| m.path_depth)
            .unwrap_or(0)
    }

    fn query_trigram(&self, tri: Trigram) -> Option<&[u32]> {
        self.query_trigram_on_disk(tri)
    }

    fn query_dir_trigram(&self, tri: Trigram) -> Option<&[u32]> {
        self.query_dir_trigram_on_disk(tri)
    }

    fn reconstruct_full_path(&self, id: FileId) -> String {
        // Prefer the stored root + relative path, but don't panic if metadata
        // is inconsistent or missing.
        self.reconstruct_absolute_path(id)
            .unwrap_or_else(|| self.get_file_name(id).to_owned())
    }
}
