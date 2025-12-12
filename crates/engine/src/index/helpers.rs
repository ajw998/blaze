use std::str;

/// Decode a UTF-8 string slice from a byte blob using (offset, len).
/// Returns "" if the range is invalid or not valid UTF-8.
#[inline]
pub fn blob_str<'a>(blob: &'a [u8], off: u32, len: u32) -> &'a str {
    let start = off as usize;

    // saturating/checked arithmetic to avoid panics on corrupt offsets
    let end = match start.checked_add(len as usize) {
        Some(end) if end <= blob.len() => end,
        _ => return "",
    };

    str::from_utf8(&blob[start..end]).unwrap_or("")
}

/// Join a stored root path and a relative path deterministically.
/// - If `rel` is empty, return `root` (owned).
/// - Ensures exactly one separator between root and rel.
/// - Does not normalize `..` or convert separators; callers should ensure `rel` uses `/`.
#[inline]
pub fn join_root_rel(root: &str, rel: &str) -> String {
    if rel.is_empty() {
        return root.to_owned();
    }

    if root.is_empty() {
        return rel.to_owned();
    }

    let root_has = root.as_bytes().last().copied() == Some(b'/');
    let rel_has = rel.as_bytes().first().copied() == Some(b'/');

    let mut out = String::with_capacity(root.len() + 1 + rel.len());
    out.push_str(root);

    match (root_has, rel_has) {
        (true, true) => out.push_str(&rel[1..]),
        (false, false) => {
            out.push('/');
            out.push_str(rel);
        }
        _ => out.push_str(rel),
    }

    out
}
