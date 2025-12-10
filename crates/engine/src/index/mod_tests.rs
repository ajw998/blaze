use super::*;
use crate::trigram::Trigram;
use memmap2::{Mmap, MmapMut};

fn build_test_index_for_trigrams() -> Index {
    // File trigrams: "abc": [1,5,10], "xyz": [42,99]
    let tri_abc = Trigram::from_bytes(b'a', b'b', b'c');
    let tri_xyz = Trigram::from_bytes(b'x', b'y', b'z');

    let file_keys = [
        TrigramKey {
            trigram: tri_abc.as_u32(),
            postings_offset: 0,
            postings_len: 3,
            _reserved: 0,
        },
        TrigramKey {
            trigram: tri_xyz.as_u32(),
            postings_offset: 3,
            postings_len: 2,
            _reserved: 0,
        },
    ];
    let file_postings: [u32; 5] = [1, 5, 10, 42, 99];

    // Dir trigrams: "dir": [7], "foo": [2,3]
    let tri_dir = Trigram::from_bytes(b'd', b'i', b'r');
    let tri_foo = Trigram::from_bytes(b'f', b'o', b'o');

    let mut dir_keys = [
        TrigramKey {
            trigram: tri_dir.as_u32(),
            postings_offset: 0,
            postings_len: 1,
            _reserved: 0,
        },
        TrigramKey {
            trigram: tri_foo.as_u32(),
            postings_offset: 1,
            postings_len: 2,
            _reserved: 0,
        },
    ];

    // IMPORTANT: sort by trigram so binary_search_by_key works
    dir_keys.sort_unstable_by_key(|k| k.trigram);

    let dir_postings: [u32; 3] = [7, 2, 3];

    // Layout in the anonymous mmap:
    // [ file_keys | file_postings | dir_keys | dir_postings ]
    let file_keys_bytes = bytemuck::cast_slice(&file_keys);
    let file_posts_bytes = bytemuck::cast_slice(&file_postings);
    let dir_keys_bytes = bytemuck::cast_slice(&dir_keys);
    let dir_posts_bytes = bytemuck::cast_slice(&dir_postings);

    let file_keys_offset = 0usize;
    let file_keys_len_bytes = file_keys_bytes.len();
    let file_posts_offset = file_keys_offset + file_keys_len_bytes;
    let file_posts_len_bytes = file_posts_bytes.len();
    let dir_keys_offset = file_posts_offset + file_posts_len_bytes;
    let dir_keys_len_bytes = dir_keys_bytes.len();
    let dir_posts_offset = dir_keys_offset + dir_keys_len_bytes;
    let dir_posts_len_bytes = dir_posts_bytes.len();

    let total_len = dir_posts_offset + dir_posts_len_bytes;

    let mut mmap_mut = MmapMut::map_anon(total_len).unwrap();
    {
        let buf = &mut mmap_mut[..];
        buf[file_keys_offset..file_keys_offset + file_keys_len_bytes]
            .copy_from_slice(file_keys_bytes);
        buf[file_posts_offset..file_posts_offset + file_posts_len_bytes]
            .copy_from_slice(file_posts_bytes);
        buf[dir_keys_offset..dir_keys_offset + dir_keys_len_bytes].copy_from_slice(dir_keys_bytes);
        buf[dir_posts_offset..dir_posts_offset + dir_posts_len_bytes]
            .copy_from_slice(dir_posts_bytes);
    }
    let mmap: Mmap = mmap_mut.make_read_only().unwrap();

    // Minimal header. Only trigram descriptors matter for these tests.
    let header = IndexHeader {
        magic: 0,
        version: 0,
        header_size: 0,
        header_crc32: 0,
        flags_bits: 0,
        file_count: 0,
        dir_count: 0,
        ext_count: 0,
        reserved: [0; 16],
        metadata: SectionDesc::new(0, 0),
        ext_table: SectionDesc::new(0, 0),
        dirs: SectionDesc::new(0, 0),
        files_meta: SectionDesc::new(0, 0),
        names_blob: SectionDesc::new(0, 0),
        trigram_keys: SectionDesc::new(file_keys_offset as u64, file_keys_len_bytes as u64),
        trigram_postings: SectionDesc::new(file_posts_offset as u64, file_posts_len_bytes as u64),
        dir_trigram_keys: SectionDesc::new(dir_keys_offset as u64, dir_keys_len_bytes as u64),
        dir_trigram_postings: SectionDesc::new(dir_posts_offset as u64, dir_posts_len_bytes as u64),
    };

    Index {
        mmap,
        header,
        ext_table: Vec::new(),
        file_metas_offset: 0,
        file_metas_len_bytes: 0,
        dirs_offset: 0,
        dirs_len_bytes: 0,
        names_blob_offset: 0,
        names_blob_len: 0,
        trigram_keys_offset: file_keys_offset,
        trigram_keys_len: file_keys_len_bytes,
        trigram_postings_offset: file_posts_offset,
        trigram_postings_len: file_posts_len_bytes,
        dir_trigram_keys_offset: dir_keys_offset,
        dir_trigram_keys_len: dir_keys_len_bytes,
        dir_trigram_postings_offset: dir_posts_offset,
        dir_trigram_postings_len: dir_posts_len_bytes,
    }
}

#[test]
fn query_trigram_on_disk_returns_correct_postings() {
    let idx = build_test_index_for_trigrams();

    let tri_abc = Trigram::from_bytes(b'a', b'b', b'c');
    let tri_xyz = Trigram::from_bytes(b'x', b'y', b'z');
    let tri_zzz = Trigram::from_bytes(b'z', b'z', b'z'); // missing

    let postings_abc = idx.query_trigram_on_disk(tri_abc).unwrap();
    assert_eq!(postings_abc, &[1, 5, 10]);

    let postings_xyz = idx.query_trigram_on_disk(tri_xyz).unwrap();
    assert_eq!(postings_xyz, &[42, 99]);

    assert!(idx.query_trigram_on_disk(tri_zzz).is_none());
}

#[test]
fn query_dir_trigram_on_disk_returns_correct_postings() {
    let idx = build_test_index_for_trigrams();

    let tri_dir = Trigram::from_bytes(b'd', b'i', b'r');
    let tri_foo = Trigram::from_bytes(b'f', b'o', b'o');
    let tri_bar = Trigram::from_bytes(b'b', b'a', b'r'); // missing

    let postings_dir = idx.query_dir_trigram_on_disk(tri_dir).unwrap();
    assert_eq!(postings_dir, &[7]);

    let postings_foo = idx.query_dir_trigram_on_disk(tri_foo).unwrap();
    assert_eq!(postings_foo, &[2, 3]);

    assert!(idx.query_dir_trigram_on_disk(tri_bar).is_none());
}
