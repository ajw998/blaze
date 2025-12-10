use crate::dsl::{CmpOp, Field, RelativeTime, TimeExpr, TimeMacro, Token, TokenKind, Value};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};

#[derive(Debug)]
enum DateParseError {
    InvalidFormat,
    InvalidDate,
}

#[derive(Debug, Clone)]
pub struct Predicate {
    pub field: Field,
    pub op: CmpOp,
    pub value: Value,
}

pub(crate) fn parse_field_predicate(
    field_name: &str,
    value_tokens: &[Token<'_>],
) -> Option<Predicate> {
    match field_name.to_ascii_lowercase().as_str() {
        "created" => parse_created_predicate(value_tokens),
        "ext" => parse_ext_predicate(value_tokens),
        "modified" => parse_modified_predicate(value_tokens),
        "size" => parse_size_predicate(value_tokens),
        _ => None,
    }
}

fn join_lexemes(tokens: &[Token<'_>]) -> String {
    let mut s = String::new();
    for t in tokens {
        s.push_str(t.lexeme);
    }
    s
}

fn parse_ext_predicate(value_tokens: &[Token<'_>]) -> Option<Predicate> {
    let tok = value_tokens.first()?;
    let mut ext = tok.lexeme.trim();

    if let Some(stripped) = ext.strip_prefix('.') {
        ext = stripped;
    }
    if ext.is_empty() {
        return None;
    }

    let ext_lower = ext.to_ascii_lowercase();

    Some(Predicate {
        field: Field::Ext,
        op: CmpOp::Eq,
        value: Value::Str(ext_lower),
    })
}

fn extract_cmp_op(s: &str) -> (CmpOp, &str) {
    if let Some(r) = s.strip_prefix(">=") {
        return (CmpOp::Ge, r);
    }
    if let Some(r) = s.strip_prefix("<=") {
        return (CmpOp::Le, r);
    }
    if let Some(r) = s.strip_prefix(">") {
        return (CmpOp::Gt, r);
    }
    if let Some(r) = s.strip_prefix("<") {
        return (CmpOp::Lt, r);
    }
    if let Some(r) = s.strip_prefix("=") {
        return (CmpOp::Eq, r);
    }
    if let Some(r) = s.strip_prefix("!=") {
        return (CmpOp::Ne, r);
    }
    (CmpOp::Eq, s)
}

fn parse_time_macro(s: &str) -> Option<TimeMacro> {
    match s {
        "today" => Some(TimeMacro::Today),
        "yesterday" => Some(TimeMacro::Yesterday),
        "this_week" | "thisweek" => Some(TimeMacro::ThisWeek),
        "last_week" | "lastweek" => Some(TimeMacro::LastWeek),
        "this_month" | "thismonth" => Some(TimeMacro::ThisMonth),
        "last_month" | "lastmonth" => Some(TimeMacro::LastMonth),
        _ => None,
    }
}

fn time_pred(field: Field, op: CmpOp, expr: TimeExpr) -> Predicate {
    Predicate {
        field,
        op,
        value: Value::Time(expr),
    }
}

fn parse_time_field_predicate(field: Field, value_tokens: &[Token<'_>]) -> Option<Predicate> {
    if value_tokens.is_empty() {
        return None;
    }

    if value_tokens.len() == 1 {
        let tok = &value_tokens[0];

        if tok.kind == TokenKind::Ident {
            let raw = tok.lexeme.to_ascii_lowercase();
            if let Some(tm) = parse_time_macro(&raw) {
                return Some(time_pred(field, CmpOp::Ge, TimeExpr::Macro(tm)));
            }
        }

        let raw = tok.lexeme.trim();
        if let Some(rt) = parse_relative_time_literal(raw) {
            return Some(time_pred(field, CmpOp::Ge, TimeExpr::Relative(rt)));
        }
    }

    let s = join_lexemes(value_tokens);
    let s = s.trim();
    let (op0, rest) = extract_cmp_op(s);
    let op = if rest == s { CmpOp::Ge } else { op0 };
    let rest = rest.trim();

    if let Ok(dt) = parse_ymd_date(rest) {
        return Some(time_pred(field, op, TimeExpr::Absolute(dt)));
    }

    if let Some(rt) = parse_relative_time_literal(rest) {
        return Some(time_pred(field, op, TimeExpr::Relative(rt)));
    }

    None
}

fn parse_modified_predicate(value_tokens: &[Token<'_>]) -> Option<Predicate> {
    parse_time_field_predicate(Field::Modified, value_tokens)
}

fn parse_created_predicate(value_tokens: &[Token<'_>]) -> Option<Predicate> {
    parse_time_field_predicate(Field::Created, value_tokens)
}

fn parse_ymd_date(s: &str) -> Result<DateTime<Utc>, DateParseError> {
    let date =
        NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| DateParseError::InvalidFormat)?;
    let dt = date
        .and_hms_opt(0, 0, 0)
        .ok_or(DateParseError::InvalidDate)?;
    Ok(Utc.from_utc_datetime(&dt))
}

/// Parses literals like '-7d', '2w', '3m', '1y'
fn parse_relative_time_literal(s: &str) -> Option<RelativeTime> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (sign, rest) = if let Some(r) = s.strip_prefix('-') {
        (-1i64, r)
    } else {
        (1i64, s)
    };

    if rest.len() < 2 {
        return None;
    }

    let (num_str, unit_str) = rest.split_at(rest.len() - 1);
    let n: i64 = num_str.trim().parse().ok()?;
    let n = n * sign;
    let unit = unit_str.to_ascii_lowercase();

    match unit.as_str() {
        "d" => Some(RelativeTime::Days(n)),
        "h" => Some(RelativeTime::Hours(n)),
        "w" => Some(RelativeTime::Weeks(n)),
        "y" => Some(RelativeTime::Years(n)),
        _ => None,
    }
}

/// Parses the size predicates
/// We implement smartcasing here, in a similar fashion to how vim does it.
/// IN general, `b` can imply bits or bytes. For the average users, we assume
/// they always want to search by bytes. For power users, it is possible they want
/// to search by bits.
/// Examples:
///     size: 5G = 5 Gigabytes
///     size: 1Mb = 1 Megabits
///     size: 1MB = 1 Megabytes
///     size: 1mb = 1 Megabytes
fn parse_size_predicate(value_tokens: &[Token<'_>]) -> Option<Predicate> {
    if value_tokens.is_empty() {
        return None;
    }

    // Reconstruct a compact string like ">10MB".
    let s = join_lexemes(value_tokens).trim().to_owned();

    if s.is_empty() {
        return None;
    }

    let (op, rest) = extract_cmp_op(&s);
    let bytes = parse_size(rest.trim())?;
    Some(Predicate {
        field: Field::Size,
        op,
        value: Value::SizeBytes(bytes),
    })
}

/// Detects if a unit suffix indicates bits using smartcasing.
///
/// This is very similar to how Vim smartcasing operates. The goal
/// is to allow the user to use units like Mega[BIT]s or Mega[Byte]s.
/// Smartcase logic:
///  1) All lowercase (i.e. `mb`, `kb`) will be interpreted as bytes
///  2) Prefix starst with uppercase + lowercase `b` (e.g., `Mb`, `Kb`) will be interpreted as bits
///  3) All uppercase (e.g., `MB`, `KB`) will be interpreted as bytes
///  4) No `b` or `B` suffix will default to bytes
#[inline]
fn is_bits_unit(unit: &[u8]) -> bool {
    let Some(&last) = unit.last() else {
        return false;
    };

    last == b'b' && unit.len() > 1 && unit[0].is_ascii_uppercase()
}

const KIB: u64 = 1024;
const MIB: u64 = KIB * 1024;
const GIB: u64 = MIB * 1024;
const TIB: u64 = GIB * 1024;

/// Parse sizes like "10MB", "500k", "5G", "10Mb" into **bytes**.
/// Prefix letters K/M/G/T (optionally with 'i' for KiB/MiB/etc.) use 1024-based multipliers.
/// No unit means raw bytes.
fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let bytes = s.as_bytes();

    // Split into number part and unit part by scanning backwards for alphabetic chars.
    let mut split = bytes.len();
    for i in (0..bytes.len()).rev() {
        if bytes[i].is_ascii_alphabetic() {
            split = i;
        } else {
            break;
        }
    }

    let (num_bytes, unit_bytes) = bytes.split_at(split);

    // Parse number from the byte slice (safe since digits are ASCII).
    let num_str = std::str::from_utf8(num_bytes).ok()?.trim();
    let num: u64 = num_str.parse().ok()?;

    if unit_bytes.is_empty() {
        return Some(num); // raw bytes, no unit
    }

    let is_bits = is_bits_unit(unit_bytes);

    let last = *unit_bytes.last().unwrap(); // safe: not empty
    let prefix_bytes = if last == b'b' || last == b'B' {
        &unit_bytes[..unit_bytes.len() - 1]
    } else {
        unit_bytes
    };

    let mut lower = prefix_bytes.to_vec();
    lower.make_ascii_lowercase();

    let factor: u64 = match lower.as_slice() {
        b"" => 1,
        b"k" | b"ki" => KIB,
        b"m" | b"mi" => MIB,
        b"g" | b"gi" => GIB,
        b"t" | b"ti" => TIB,
        _ => return None,
    };

    let value = num.saturating_mul(factor);

    if is_bits {
        Some(value / 8)
    } else {
        Some(value)
    }
}

#[cfg(test)]
#[path = "predicates_tests.rs"]
mod tests;
