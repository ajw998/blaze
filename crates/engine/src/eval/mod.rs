use chrono::{DateTime, Utc};

mod helpers;
mod planner;
mod predicates;
mod rank;
mod text;

pub use helpers::{diff_sorted, intersect_adaptive, intersect_sorted, union_sorted};
use log::debug;
use planner::{estimate_cost, estimate_cost_simple};
use predicates::eval_predicate;
pub use rank::*;

use crate::{
    dsl::{LeafExpr, Query, QueryExpr, TextTerm},
    eval::{
        planner::{Cost, estimate_text_term_cost},
        text::filter_candidates_by_all_terms,
    },
    index::{FileId, IndexReader},
};

pub struct QueryEngine<'a, I: IndexReader + Sync> {
    index: &'a I,
}

impl<'a, I: IndexReader + Sync> QueryEngine<'a, I> {
    pub fn new(index: &'a I) -> Self {
        Self { index }
    }

    pub fn eval_query(&self, query: &Query) -> Vec<FileId> {
        let timestamp = Utc::now();
        let candidates: Vec<FileId> = (0..self.index.get_file_count() as FileId).collect();
        self.eval_expr(&query.expr, &candidates, timestamp)
    }

    fn eval_expr(
        &self,
        expr: &QueryExpr,
        candidates: &[FileId],
        timestamp: DateTime<Utc>,
    ) -> Vec<FileId> {
        match expr {
            QueryExpr::Leaf(leaf) => self.eval_leaf(leaf, candidates, timestamp),

            QueryExpr::And(children) => {
                if children.is_empty() {
                    return candidates.to_vec();
                }

                // Detect pure-text conjunction: AND of only Text leaves.
                let text_terms: Vec<&TextTerm> = children
                    .iter()
                    .filter_map(|c| match c {
                        QueryExpr::Leaf(LeafExpr::Text(t)) => Some(t),
                        _ => None,
                    })
                    .collect();

                if text_terms.len() >= 2 && text_terms.len() == children.len() {
                    return self.eval_pure_text_conjunction(&text_terms, candidates, timestamp);
                }

                let mut ordered = children.clone();

                let use_index_costs = text_terms.len() >= 2;
                if use_index_costs {
                    ordered.sort_by_cached_key(|child| estimate_cost(self.index, child));
                } else {
                    ordered.sort_by_key(estimate_cost_simple);
                }

                let mut current = candidates.to_vec();
                for child in ordered {
                    if current.is_empty() {
                        break;
                    }
                    let subset = self.eval_expr(&child, &current, timestamp);
                    current = subset;
                }
                current
            }

            QueryExpr::Or(children) => {
                if children.is_empty() {
                    return Vec::new();
                }

                // We maintain the invariant that all candidate sets are sorted.
                let mut acc: Vec<FileId> = Vec::new();

                for child in children {
                    let subset = self.eval_expr(child, candidates, timestamp);
                    if acc.is_empty() {
                        // Fast path: first non-empty subset, take it as-is
                        acc = subset;
                    } else if !subset.is_empty() {
                        acc = union_sorted(&acc, &subset);
                    }
                }

                acc
            }

            QueryExpr::Not(inner) => {
                let inner_ids = self.eval_expr(inner, candidates, timestamp);
                if inner_ids.is_empty() {
                    candidates.to_vec()
                } else {
                    diff_sorted(candidates, &inner_ids)
                }
            }
        }
    }

    /// Leaf evaluation: delegate to text or predicate subsystem.
    fn eval_leaf(
        &self,
        leaf: &LeafExpr,
        candidates: &[FileId],
        timestamp: DateTime<Utc>,
    ) -> Vec<FileId> {
        match leaf {
            LeafExpr::Text(term) => text::eval_text_term(self.index, term, candidates),
            LeafExpr::Predicate(pred) => eval_predicate(self.index, pred, candidates, timestamp),
        }
    }
    /// Optimised evaluation for AND of only text terms.
    ///
    /// Strategy:
    /// - Use trigram stats to estimate cost for each term.
    /// - Avoid seeding from "broad" terms that hit most of the index when we
    ///   have a more selective alternative.
    /// - Seed from the most selective non-broad term, then
    ///   verify *all* terms in a single pass over the candidate paths.
    fn eval_pure_text_conjunction(
        &self,
        terms: &[&TextTerm],
        candidates: &[FileId],
        _timestamp: DateTime<Utc>,
    ) -> Vec<FileId> {
        // Degenerate cases: nothing to do.
        if candidates.is_empty() {
            return Vec::new();
        }
        if terms.is_empty() {
            // AND over no terms is identity: keep current candidates.
            return candidates.to_vec();
        }

        let file_count = self.index.get_file_count();
        if file_count == 0 {
            return Vec::new();
        }

        // "Broad" threshold: a term whose effective cost exceeds this is considered
        // too broad to seed from if we have any more selective alternative.
        let broad_threshold: u64 = ((file_count as f64) * 0.6) as u64;

        // Compute costs and detect impossible/broad terms.
        let mut term_costs: Vec<(Cost, &TextTerm, bool)> = Vec::with_capacity(terms.len());

        for &term in terms {
            let cost = estimate_text_term_cost(self.index, term);

            // Perfect anchor: this term cannot match any file in the index.
            // In an AND conjunction, that makes the whole expression unsatisfiable.
            if cost == Cost::ZERO {
                return Vec::new();
            }

            // Broad if:
            //   - cost is above 60% of total files, or
            //   - the estimator has already classified it as ultra-broad / linear-scan.
            let is_broad =
                cost.0 > broad_threshold || cost == Cost::VERY_BAD || cost == Cost::LINEAR_SCAN;

            term_costs.push((cost, term, is_broad));
        }

        // Sort by cost (ascending) – most selective first.
        term_costs.sort_unstable_by_key(|(cost, _, _)| *cost);

        #[cfg(debug_assertions)]
        {
            debug!("[DEBUG] Pure-text AND term costs:");
            for (cost, term, is_broad) in &term_costs {
                debug!(
                    "  '{}': cost={} {}",
                    term.text,
                    cost.0,
                    if *is_broad { "(BROAD)" } else { "" }
                );
            }
        }

        // Choose seed:
        // - Prefer the most selective *non-broad* term.
        // - If all are broad, fall back to the cheapest term.
        let seed_term: &TextTerm =
            if let Some((_, term, _)) = term_costs.iter().find(|(_, _, is_broad)| !*is_broad) {
                *term
            } else {
                term_costs[0].1
            };

        #[cfg(debug_assertions)]
        debug!("[DEBUG] Pure-text AND: seeding from '{}'", seed_term.text);

        // Evaluate the seed term with the full text engine (trigram + verification),
        // but restricted to the current candidate set.
        let seed_candidates = text::eval_text_term(self.index, seed_term, candidates);

        if seed_candidates.is_empty() {
            return Vec::new();
        }

        #[cfg(debug_assertions)]
        debug!(
            "[DEBUG] Pure-text AND: seed '{}' produced {} candidates",
            seed_term.text,
            seed_candidates.len()
        );

        // Single-pass verification: check *all* terms (including the seed) against each
        // candidate path exactly once (filename first, then full path if needed).
        let filtered = filter_candidates_by_all_terms(self.index, terms, &seed_candidates);

        #[cfg(debug_assertions)]
        debug!(
            "[DEBUG] Pure-text AND: after all-terms filter: {} results",
            filtered.len()
        );

        filtered
    }
}
