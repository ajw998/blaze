use std::cmp::Ordering;

use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};

use crate::{CmpOp, RelativeTime, TimeExpr, TimeMacro};

/// Adaptive intersection into `out`: linear vs galloping.
#[inline]
pub fn intersect_adaptive_into<T: Ord + Copy>(a: &[T], b: &[T], out: &mut Vec<T>) {
    out.clear();

    if a.is_empty() || b.is_empty() {
        return;
    }

    let (small, large) = if a.len() < b.len() { (a, b) } else { (b, a) };

    if small.len() * 8 < large.len() {
        galloping_intersect_into(small, large, out);
    } else {
        intersect_sorted_into(a, b, out);
    }
}

/// Linear merge intersection into `out`.
#[inline]
pub fn intersect_sorted_into<T: Ord + Copy>(a: &[T], b: &[T], out: &mut Vec<T>) {
    out.clear();

    if a.is_empty() || b.is_empty() {
        return;
    }

    out.reserve(a.len().min(b.len()));

    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Ordering::Less => i += 1,
            Ordering::Greater => j += 1,
            Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
}

/// Owning wrapper around `intersect_sorted_into`.
#[inline]
pub fn intersect_sorted<T: Ord + Copy>(a: &[T], b: &[T]) -> Vec<T> {
    let mut out = Vec::with_capacity(a.len().min(b.len()));
    intersect_sorted_into(a, b, &mut out);
    out
}

/// Owning wrapper around `intersect_adaptive_into`.
#[inline]
pub fn intersect_adaptive<T: Ord + Copy>(a: &[T], b: &[T]) -> Vec<T> {
    let mut out = Vec::with_capacity(a.len().min(b.len()));
    intersect_adaptive_into(a, b, &mut out);
    out
}

/// Galloping intersection into `out` (skewed sizes).
fn galloping_intersect_into<T: Ord + Copy>(small: &[T], large: &[T], out: &mut Vec<T>) {
    out.reserve(small.len());
    let mut large_idx = 0;

    for &elem in small {
        // Exponential search
        let mut step = 1;
        while large_idx + step < large.len() && large[large_idx + step] < elem {
            large_idx += step;
            step *= 2;
        }

        // Binary search in [large_idx, large_idx + step]
        let search_end = (large_idx + step + 1).min(large.len());
        match large[large_idx..search_end].binary_search(&elem) {
            Ok(offset) => {
                out.push(elem);
                large_idx += offset + 1;
            }
            Err(offset) => {
                large_idx += offset;
            }
        }

        if large_idx >= large.len() {
            break;
        }
    }
}

/// Union of two sorted slices (removes duplicates).
#[inline]
pub fn union_sorted<T: Ord + Copy>(a: &[T], b: &[T]) -> Vec<T> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);

    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Ordering::Less => {
                out.push(a[i]);
                i += 1;
            }
            Ordering::Greater => {
                out.push(b[j]);
                j += 1;
            }
            Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }

    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
    out
}

/// Difference of two sorted slices.
#[inline]
pub fn diff_sorted<T: Ord + Copy>(base: &[T], sub: &[T]) -> Vec<T> {
    let mut out = Vec::with_capacity(base.len());
    let (mut i, mut j) = (0, 0);

    while i < base.len() && j < sub.len() {
        match base[i].cmp(&sub[j]) {
            Ordering::Less => {
                out.push(base[i]);
                i += 1;
            }
            Ordering::Greater => {
                j += 1;
            }
            Ordering::Equal => {
                i += 1;
                j += 1;
            }
        }
    }

    out.extend_from_slice(&base[i..]);
    out
}

pub fn cmp_str_ci(lhs: &str, rhs: &str, op: CmpOp) -> bool {
    let eq = lhs.eq_ignore_ascii_case(rhs);
    match op {
        CmpOp::Eq => eq,
        CmpOp::Ne => !eq,
        // Lexical comparison doesn't make sense for extensions
        CmpOp::Gt | CmpOp::Ge | CmpOp::Lt | CmpOp::Le => false,
    }
}

pub fn cmp_u64(lhs: u64, rhs: u64, op: CmpOp) -> bool {
    match op {
        CmpOp::Eq => lhs == rhs,
        CmpOp::Ne => lhs != rhs,
        CmpOp::Gt => lhs > rhs,
        CmpOp::Ge => lhs >= rhs,
        CmpOp::Lt => lhs < rhs,
        CmpOp::Le => lhs <= rhs,
    }
}

pub fn cmp_i64(lhs: i64, rhs: i64, op: CmpOp) -> bool {
    match op {
        CmpOp::Eq => lhs == rhs,
        CmpOp::Ne => lhs != rhs,
        CmpOp::Gt => lhs > rhs,
        CmpOp::Ge => lhs >= rhs,
        CmpOp::Lt => lhs < rhs,
        CmpOp::Le => lhs <= rhs,
    }
}

pub fn resolve_time_expr(expr: &TimeExpr, now: DateTime<Utc>) -> i64 {
    match expr {
        TimeExpr::Absolute(dt) => dt.timestamp(),
        TimeExpr::Relative(rel) => resolve_relative_time(rel, now),
        TimeExpr::Macro(mac) => resolve_time_macro(mac, now),
    }
}

fn resolve_relative_time(rel: &RelativeTime, now: DateTime<Utc>) -> i64 {
    let duration = match rel {
        RelativeTime::Days(n) => Duration::days(*n),
        RelativeTime::Hours(n) => Duration::hours(*n),
        RelativeTime::Weeks(n) => Duration::weeks(*n),
        RelativeTime::Years(n) => Duration::days(*n * 365),
    };
    (now - duration).timestamp()
}

fn resolve_time_macro(mac: &TimeMacro, now: DateTime<Utc>) -> i64 {
    match mac {
        TimeMacro::Today => start_of_day(now).timestamp(),
        TimeMacro::Yesterday => start_of_day(now - Duration::days(1)).timestamp(),
        TimeMacro::ThisWeek => start_of_week(now).timestamp(),
        TimeMacro::LastWeek => start_of_week(now - Duration::weeks(1)).timestamp(),
        TimeMacro::ThisMonth => start_of_month(now).timestamp(),
        TimeMacro::LastMonth => {
            let prev = if now.month() == 1 {
                Utc.with_ymd_and_hms(now.year() - 1, 12, 1, 0, 0, 0)
                    .single()
                    .unwrap_or(now)
            } else {
                Utc.with_ymd_and_hms(now.year(), now.month() - 1, 1, 0, 0, 0)
                    .single()
                    .unwrap_or(now)
            };
            prev.timestamp()
        }
    }
}

fn start_of_day(dt: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(dt.year(), dt.month(), dt.day(), 0, 0, 0)
        .single()
        .unwrap_or(dt)
}

fn start_of_week(dt: DateTime<Utc>) -> DateTime<Utc> {
    let weekday = dt.weekday().num_days_from_monday();
    start_of_day(dt - Duration::days(weekday as i64))
}

fn start_of_month(dt: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(dt.year(), dt.month(), 1, 0, 0, 0)
        .single()
        .unwrap_or(dt)
}

#[cfg(test)]
#[path = "helpers_tests.rs"]
mod tests;
