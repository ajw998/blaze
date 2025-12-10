use super::*;

fn assert_sorted_trigrams(tris: &[Trigram]) {
    for w in tris.windows(2) {
        assert!(w[0] <= w[1], "Trigrams not sorted: {:?} > {:?}", w[0], w[1]);
    }
}

#[test]
fn ascii_fold_lowercases_ascii_letters_and_preserves_others() {
    for (upper, lower) in ('A'..='Z').zip('a'..='z') {
        assert_eq!(ascii_fold(upper as u8), lower as u8);
        assert_eq!(ascii_fold(lower as u8), lower as u8);
    }

    // Digits and punctuation
    assert_eq!(ascii_fold(b'0'), b'0');
    assert_eq!(ascii_fold(b'_'), b'_');

    // Non-ASCII byte must be unchanged
    assert_eq!(ascii_fold(0xFF), 0xFF);
}

#[test]
fn normalize_for_trigram_str_handles_ascii_and_non_ascii() {
    let s = "AbC中文123";
    let bytes = s.as_bytes();
    let norm = normalize_for_trigram_str(s);

    assert_eq!(norm.len(), bytes.len());

    for (src, dst) in bytes.iter().zip(norm.iter()) {
        if src.is_ascii_uppercase() {
            assert_eq!(*dst, src.to_ascii_lowercase());
        } else {
            assert_eq!(*dst, *src);
        }
    }
}

#[test]
fn normalize_for_trigram_bytes_handles_arbitrary_bytes() {
    let input: [u8; 4] = [0xFF, b'A', 0x00, b'z'];
    let norm = normalize_for_trigram_bytes(&input);

    assert_eq!(norm.len(), input.len());
    // Non-ASCII preserved
    assert_eq!(norm[0], 0xFF);
    // ASCII uppercase lowercased
    assert_eq!(norm[1], b'a');
    // Zero preserved
    assert_eq!(norm[2], 0x00);
    // ASCII lowercase preserved
    assert_eq!(norm[3], b'z');
}

#[test]
fn trigram_from_bytes_and_to_bytes_roundtrip() {
    let cases = &[
        (b'a', b'b', b'c'),
        (0u8, 0u8, 0u8),
        (255u8, 1u8, 2u8),
        (b'X', b'Y', b'Z'),
    ];

    for &(b0, b1, b2) in cases {
        let tri = Trigram::from_bytes(b0, b1, b2);
        let bytes = tri.to_bytes();
        assert_eq!(bytes, [b0, b1, b2]);

        let v = tri.as_u32();
        let tri2 = Trigram::from_u32(v);
        assert_eq!(tri, tri2);
    }
}

#[test]
fn build_trigrams_for_string_sliding_windows_ascii() {
    // Mixed case to exercise normalization
    let s = "AbCd";
    let tris = build_trigrams_for_string(s);

    assert_eq!(tris.len(), 2);

    let expected = vec![
        Trigram::from_bytes(b'a', b'b', b'c'),
        Trigram::from_bytes(b'b', b'c', b'd'),
    ];

    assert_sorted_trigrams(&tris);
    assert_eq!(tris, expected);
}

#[test]
fn build_trigrams_for_string_deduplicates_trigrams() {
    // "AAAA" to "aaaa" to windows: "aaa", "aaa"
    let s = "AAAA";
    let tris = build_trigrams_for_string(s);

    assert_eq!(tris.len(), 1);
    assert_eq!(tris[0], Trigram::from_bytes(b'a', b'a', b'a'));
}

#[test]
fn build_trigrams_for_string_short_inputs_produce_empty() {
    assert!(build_trigrams_for_string("").is_empty());
    assert!(build_trigrams_for_string("A").is_empty());
    assert!(build_trigrams_for_string("Ab").is_empty());
}

#[test]
fn build_trigrams_for_bytes_short_inputs_produce_empty() {
    assert!(build_trigrams_for_bytes(&[]).is_empty());
    assert!(build_trigrams_for_bytes(&[b'a']).is_empty());
    assert!(build_trigrams_for_bytes(&[b'a', b'b']).is_empty());
}

#[test]
fn build_trigrams_for_bytes_non_ascii_roundtrip() {
    // "中Ab" in UTF-8: 3 bytes for '中', 1 for 'A', 1 for 'b' = total 5 bytes
    let s = "中Ab";
    let bytes = s.as_bytes();
    assert_eq!(bytes.len(), 5);

    let norm = normalize_for_trigram_bytes(bytes);
    assert_eq!(norm.len(), bytes.len());

    let tris = build_trigrams_for_bytes(bytes);
    assert_eq!(tris.len(), 3);

    let mut expected = Vec::new();
    for win in norm.windows(3) {
        expected.push(Trigram::from_bytes(win[0], win[1], win[2]));
    }
    expected.sort_unstable();
    expected.dedup();

    assert_sorted_trigrams(&tris);
    assert_eq!(tris, expected);
}

#[test]
fn build_trigrams_for_string_matches_bytes_for_utf8() {
    let s = "中Ab文";
    let t1 = build_trigrams_for_string(s);
    let t2 = build_trigrams_for_bytes(s.as_bytes());
    assert_eq!(t1, t2);
}

#[test]
fn build_trigrams_for_bytes_handles_arbitrary_non_utf8() {
    let bytes: [u8; 3] = [0xFF, 0x00, 0x7F];
    let tris = build_trigrams_for_bytes(&bytes);

    assert_eq!(tris.len(), 1);
    assert_eq!(tris[0].to_bytes(), bytes);
}

#[test]
fn build_query_trigrams_short_queries_return_empty() {
    assert!(build_query_trigrams("").is_empty());
    assert!(build_query_trigrams("a").is_empty());
    assert!(build_query_trigrams("ab").is_empty());
}

#[test]
fn build_query_trigrams_len3_full_coverage() {
    let tris = build_query_trigrams("Abc");
    assert_eq!(tris.len(), 1);
    assert_eq!(tris[0], Trigram::from_bytes(b'a', b'b', b'c'));
}

#[test]
fn build_query_trigrams_len4_two_trigrams_cover_entire_span() {
    // "AbCd" to "abcd"
    let tris = build_query_trigrams("AbCd");

    assert_eq!(tris.len(), 2);
    assert_sorted_trigrams(&tris);
    assert!(tris.contains(&Trigram::from_bytes(b'a', b'b', b'c')));
    assert!(tris.contains(&Trigram::from_bytes(b'b', b'c', b'd')));
}

#[test]
fn build_query_trigrams_len5_suffix_included() {
    let tris = build_query_trigrams("abcde");

    // Sliding windows: "abc", "bcd", "cde"
    // Query strategy: "abc", "cde"
    assert_eq!(tris.len(), 2);
    assert_sorted_trigrams(&tris);
    assert!(tris.contains(&Trigram::from_bytes(b'a', b'b', b'c')));
    assert!(tris.contains(&Trigram::from_bytes(b'c', b'd', b'e')));
}

#[test]
fn build_query_trigrams_len6_two_non_overlapping() {
    let tris = build_query_trigrams("config");

    // Sliding: "con", "onf", "nfi", "fig"
    // Query strategy: "con", "fig"
    assert_eq!(tris.len(), 2);
    assert_sorted_trigrams(&tris);
    assert!(tris.contains(&Trigram::from_bytes(b'c', b'o', b'n')));
    assert!(tris.contains(&Trigram::from_bytes(b'f', b'i', b'g')));
}

#[test]
fn build_query_trigrams_len7_suffix_included() {
    let tris = build_query_trigrams("abcdefg");

    // Sliding: abc, bcd, cde, def, efg
    // Query strategy: abc, def, efg
    assert_eq!(tris.len(), 3);
    assert_sorted_trigrams(&tris);
    assert!(tris.contains(&Trigram::from_bytes(b'a', b'b', b'c')));
    assert!(tris.contains(&Trigram::from_bytes(b'd', b'e', b'f')));
    assert!(tris.contains(&Trigram::from_bytes(b'e', b'f', b'g')));
}

#[test]
fn build_query_trigrams_len8_suffix_included() {
    let tris = build_query_trigrams("abcdefgh");

    // Sliding: abc, bcd, cde, def, efg, fgh
    // Query strategy: abc, def, fgh
    assert_eq!(tris.len(), 3);
    assert_sorted_trigrams(&tris);
    assert!(tris.contains(&Trigram::from_bytes(b'a', b'b', b'c')));
    assert!(tris.contains(&Trigram::from_bytes(b'd', b'e', b'f')));
    assert!(tris.contains(&Trigram::from_bytes(b'f', b'g', b'h')));
}

#[test]
fn build_query_trigrams_deduplicates_trigrams() {
    // Sliding: "aaa", "aaa"
    // Query strategy: positions 0 and suffix at 1
    let tris = build_query_trigrams("AAAA");

    assert_eq!(tris.len(), 1);
    assert_eq!(tris[0], Trigram::from_bytes(b'a', b'a', b'a'));
}

#[test]
fn query_trigrams_are_subset_of_full_trigrams() {
    let samples = [
        "commands",
        "config",
        "lib_controller",
        "中Ab文",
        "aaaaaa",
        "xyz",
    ];

    for s in samples.iter() {
        let full = build_trigrams_for_string(s);
        let q = build_query_trigrams(s);

        for tri in &q {
            assert!(
                full.contains(tri),
                "Query trigram {:?} not present in full trigram set for {:?}",
                tri,
                s
            );
        }
    }
}
