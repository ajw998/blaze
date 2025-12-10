// TODO: See whether we can refactor the duplicate code
use crate::{
    Field, IndexReader, LeafExpr, Predicate, QueryExpr, TextTerm,
    trigram::{Trigram, build_trigrams_for_string},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cost(pub u64);

impl Cost {
    /// Perfect anchor (impossible term, kills all candidates immediately).
    pub const ZERO: Cost = Cost(0);
    /// Ultra-broad / must-do-linear-scan; avoid as driver if possible.
    pub const VERY_BAD: Cost = Cost(u64::MAX / 3);
    /// Worse case. Will have to scan the full candidate set
    pub const LINEAR_SCAN: Cost = Cost(u64::MAX / 2);
}

impl std::ops::Add for Cost {
    type Output = Cost;

    #[inline]
    fn add(self, rhs: Cost) -> Cost {
        Cost(self.0.saturating_add(rhs.0))
    }
}

pub fn estimate_cost_simple(expr: &QueryExpr) -> Cost {
    match expr {
        QueryExpr::Leaf(LeafExpr::Predicate(pred)) => estimate_predicate_cost_simple(pred),
        QueryExpr::Leaf(LeafExpr::Text(term)) => estimate_text_cost_simple(term),
        QueryExpr::Not(inner) => estimate_cost_simple(inner) + Cost(1),
        QueryExpr::And(children) | QueryExpr::Or(children) => children
            .iter()
            .map(estimate_cost_simple)
            .min()
            .unwrap_or(Cost(5)),
    }
}

fn estimate_predicate_cost_simple(pred: &Predicate) -> Cost {
    match pred.field {
        Field::Ext => Cost(10),
        Field::Size => Cost(20),
        Field::Created => Cost(25),
        Field::Modified => Cost(25),
    }
}

fn estimate_text_cost_simple(term: &TextTerm) -> Cost {
    let len = term.text.chars().count() as u64;
    if len < 3 {
        // Length < 3 means no trigram, fallback to linear scan
        return Cost::LINEAR_SCAN;
    }
    let capped = len.min(40);

    Cost(10 + (30 - capped))
}

// Index-aware text cost estimation

pub fn estimate_cost<I: IndexReader>(index: &I, expr: &QueryExpr) -> Cost {
    // Get the universe first, then distill down to a smaller subset
    let candidates = index.get_file_count();
    estimate_cost_internal(index, expr, candidates)
}

fn estimate_cost_internal<I: IndexReader>(
    index: &I,
    expr: &QueryExpr,
    candidate_count: usize,
) -> Cost {
    match expr {
        QueryExpr::Leaf(LeafExpr::Predicate(pred)) => {
            estimate_predicate_cost::<I>(pred, candidate_count)
        }
        QueryExpr::Leaf(LeafExpr::Text(term)) => estimate_text_term_cost(index, term),
        QueryExpr::Not(inner) => estimate_cost_internal(index, inner, candidate_count) + Cost(1),
        QueryExpr::And(children) | QueryExpr::Or(children) => children
            .iter()
            .map(|c| estimate_cost_internal(index, c, candidate_count))
            .min()
            .unwrap_or(Cost(5)),
    }
}

fn estimate_predicate_cost<I: IndexReader>(pred: &Predicate, candidate_count: usize) -> Cost {
    let n = candidate_count as u64;

    match pred.field {
        Field::Ext => Cost(n),
        Field::Size => Cost(2 * n),
        Field::Created | Field::Modified => Cost(3 * n),
    }
}

pub fn estimate_text_term_cost<I: IndexReader>(index: &I, term: &TextTerm) -> Cost {
    let search_text = term.text.as_str();
    let trigrams: Vec<Trigram> = build_trigrams_for_string(search_text);

    if trigrams.is_empty() {
        return Cost::LINEAR_SCAN;
    }

    let file_count = index.get_file_count() as u64;
    let dir_count = index.dir_count() as u64;

    if file_count == 0 {
        return Cost::ZERO;
    }

    let file_threshold = (file_count as f64 * 0.30) as usize;
    let dir_threshold = (dir_count as f64 * 0.30) as usize;

    let mut file_cost: u64 = 0;
    let mut dir_cost: u64 = 0;
    let mut impossible = false;

    for tri in &trigrams {
        let f_len = index
            .query_trigram(*tri)
            .map_or(0usize, |slice| slice.len());
        let d_len = index
            .query_dir_trigram(*tri)
            .map_or(0usize, |slice| slice.len());

        // Trigram literally never appears anywhere
        if f_len == 0 && d_len == 0 {
            impossible = true;
            break;
        }

        if f_len > 0 && f_len <= file_threshold {
            file_cost += f_len as u64;
        }

        if d_len > 0 && d_len <= dir_threshold {
            dir_cost += d_len as u64;
        }
    }

    if impossible {
        // Perfect anchor
        return Cost::ZERO;
    }

    if file_cost > 0 {
        // Tier 1: filename-selective term. cost = number of candidate files
        // we need to touch when using this as a driver.
        return Cost(file_cost);
    }

    if dir_cost > 0 {
        // Tier 2: directory-only term. Always worse than any file-based term,
        return Cost(file_count + dir_cost);
    }

    // Tier 3: ultra-broad: trigrams appear everywhere and don't help prune.
    // We should actively avoid this
    Cost::VERY_BAD
}
