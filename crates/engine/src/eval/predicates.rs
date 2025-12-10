use chrono::{DateTime, Utc};

use crate::{
    Field, FileId, IndexReader, Predicate, Value,
    eval::helpers::{cmp_i64, cmp_str_ci, cmp_u64, resolve_time_expr},
};

pub fn eval_predicate<I: IndexReader>(
    index: &I,
    pred: &Predicate,
    candidates: &[FileId],
    now: DateTime<Utc>,
) -> Vec<FileId> {
    match pred.field {
        Field::Ext => eval_predicate_ext(index, pred, candidates),
        Field::Size => eval_predicate_size(index, pred, candidates),
        Field::Modified => eval_predicate_modified(index, pred, candidates, now),
        Field::Created => eval_predicate_created(index, pred, candidates, now),
    }
}

fn eval_predicate_size<I: IndexReader>(
    index: &I,
    pred: &Predicate,
    candidates: &[u32],
) -> Vec<u32> {
    let Value::SizeBytes(threshold) = pred.value else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for &fid in candidates {
        let size = index.get_file_size(fid);
        if cmp_u64(size, threshold, pred.op) {
            out.push(fid);
        }
    }
    out
}

fn eval_predicate_ext<I: IndexReader>(index: &I, pred: &Predicate, candidates: &[u32]) -> Vec<u32> {
    let Value::Str(ref wanted) = pred.value else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for &fid in candidates {
        let ext = index.get_file_ext(fid);
        if cmp_str_ci(ext, wanted, pred.op) {
            out.push(fid);
        }
    }
    out
}

// TODO: Check whether we can abstract the functions below
fn eval_predicate_created<I: IndexReader>(
    index: &I,
    pred: &Predicate,
    candidates: &[u32],
    now: DateTime<Utc>,
) -> Vec<u32> {
    let Value::Time(ref time_expr) = pred.value else {
        return Vec::new();
    };

    let threshold_secs = resolve_time_expr(time_expr, now);

    let mut out = Vec::new();
    for &fid in candidates {
        let ctime = index.get_file_created_epoch(fid);
        if cmp_i64(ctime, threshold_secs, pred.op) {
            out.push(fid);
        }
    }
    out
}

fn eval_predicate_modified<I: IndexReader>(
    index: &I,
    pred: &Predicate,
    candidates: &[u32],
    now: DateTime<Utc>,
) -> Vec<u32> {
    let Value::Time(ref time_expr) = pred.value else {
        return Vec::new();
    };

    let threshold_secs = resolve_time_expr(time_expr, now);

    let mut out = Vec::new();
    for &fid in candidates {
        let ctime = index.get_file_modified_epoch(fid);
        if cmp_i64(ctime, threshold_secs, pred.op) {
            out.push(fid);
        }
    }
    out
}
