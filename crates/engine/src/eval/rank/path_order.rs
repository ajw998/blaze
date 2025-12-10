use crate::{FileId, IndexReader, LeafExpr, Query, QueryExpr};

/// Check if terms appear in order within a path.
///
/// Terms can match within the same component or across components,
/// but must maintain left-to-right order. `blaze`'s goal is not the reimplement
/// fuzzy search. The user MUST in their mind have some semblance of the file name
/// they are looking for.
/// 1. "foo/bar", search=["foo", "bar"] // match
/// 2. "foo/baz/bar", search=["foo", "bar"] // match
/// 2. "bar/baz/foo", search=["foo", "bar"] // not match
pub fn terms_match_in_order(path_lower: &str, terms: &[&str]) -> bool {
    if terms.is_empty() {
        return true;
    }

    let mut search_start = 0;

    for term in terms {
        if term.is_empty() {
            continue;
        }

        match path_lower[search_start..].find(term) {
            Some(pos) => {
                // Move search start past this match
                search_start += pos + term.len();
            }
            None => return false,
        }
    }

    true
}

/// Extract plain text terms from query in order (for path-order filtering).
///
/// Only collects bare text terms, ignoring field predicates like `ext:rs`.
/// Terms are lowercased for case-insensitive matching.
fn collect_text_terms_in_order(expr: &QueryExpr, out: &mut Vec<String>) {
    match expr {
        QueryExpr::Leaf(LeafExpr::Text(term)) => {
            let t = term.text.trim().to_lowercase();
            if !t.is_empty() {
                out.push(t);
            }
        }
        QueryExpr::And(children) => {
            // For AND, collect terms in order
            for child in children {
                collect_text_terms_in_order(child, out);
            }
        }
        QueryExpr::Or(children) => {
            // For OR, we can't enforce strict ordering across branches.
            // Collect from first branch as a heuristic.
            if let Some(first) = children.first() {
                collect_text_terms_in_order(first, out);
            }
        }
        QueryExpr::Not(_) => {
            // Don't include negated terms in order check
        }
        QueryExpr::Leaf(LeafExpr::Predicate(_)) => {
            // Predicates don't participate in path-order matching
        }
    }
}

/// Apply path-order filter to results.
///
/// For queries with 2+ text terms, ensures terms appear in order in the path.
/// This is applied AFTER the boolean evaluation, before ranking.
pub fn apply_path_order_filter<I: IndexReader>(
    index: &I,
    query: &Query,
    file_ids: Vec<FileId>,
) -> Vec<FileId> {
    let mut terms = Vec::new();
    collect_text_terms_in_order(&query.expr, &mut terms);

    // Only apply ordering constraint if we have multiple terms
    if terms.len() < 2 {
        return file_ids;
    }

    let term_refs: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();

    file_ids
        .into_iter()
        .filter(|&fid| {
            let path = index.reconstruct_full_path(fid).to_lowercase();
            terms_match_in_order(&path, &term_refs)
        })
        .collect()
}

// #[cfg(test)]
// #[path = "path_order_tests.rs"]
// mod tests;
