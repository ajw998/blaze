use super::*;
use chrono::{Datelike, Timelike};

#[test]
fn extract_cmp_op_basic_cases() {
    let cases: &[(&str, CmpOp, &str)] = &[
        (">=10", CmpOp::Ge, "10"),
        ("<=10", CmpOp::Le, "10"),
        (">10", CmpOp::Gt, "10"),
        ("<10", CmpOp::Lt, "10"),
        ("=10", CmpOp::Eq, "10"),
        ("10", CmpOp::Eq, "10"),
    ];

    for (input, expected_op, expected_rest) in cases {
        let (op, rest) = extract_cmp_op(input);
        assert_eq!(op, *expected_op, "input: {:?}", input);
        assert_eq!(rest, *expected_rest, "input: {:?}", input);
    }
}

#[test]
fn parse_time_macro_recognizes_macros() {
    let cases: &[(&str, Option<TimeMacro>)] = &[
        ("today", Some(TimeMacro::Today)),
        ("yesterday", Some(TimeMacro::Yesterday)),
        ("this_week", Some(TimeMacro::ThisWeek)),
        ("thisweek", Some(TimeMacro::ThisWeek)),
        ("last_week", Some(TimeMacro::LastWeek)),
        ("lastweek", Some(TimeMacro::LastWeek)),
        ("this_month", Some(TimeMacro::ThisMonth)),
        ("thismonth", Some(TimeMacro::ThisMonth)),
        ("last_month", Some(TimeMacro::LastMonth)),
        ("lastmonth", Some(TimeMacro::LastMonth)),
        ("unknown", None),
        ("", None),
    ];

    for (input, expected) in cases {
        let got = parse_time_macro(input);
        assert_eq!(got, *expected, "input: {:?}", input);
    }
}

#[test]
fn parse_ymd_date_parses_valid_date_at_midnight_utc() {
    let dt = parse_ymd_date("2025-11-30").expect("valid date");
    assert_eq!(dt.year(), 2025);
    assert_eq!(dt.month(), 11);
    assert_eq!(dt.day(), 30);
    // midnight UTC
    assert_eq!(dt.hour(), 0);
    assert_eq!(dt.minute(), 0);
    assert_eq!(dt.second(), 0);
}

#[test]
fn parse_ymd_date_rejects_invalid_format() {
    match parse_ymd_date("not-a-date") {
        Err(DateParseError::InvalidFormat) => {}
        other => panic!("expected InvalidFormat, got {:?}", other),
    }

    // Structurally OK but invalid date.
    match parse_ymd_date("2025-02-30") {
        Err(DateParseError::InvalidFormat) => {}
        other => panic!("expected InvalidFormat for invalid date, got {:?}", other),
    }
}

#[test]
fn parse_relative_time_literal_parses_supported_units() {
    let cases: &[(&str, Option<RelativeTime>)] = &[
        ("7d", Some(RelativeTime::Days(7))),
        ("-7d", Some(RelativeTime::Days(-7))),
        ("3h", Some(RelativeTime::Hours(3))),
        ("2w", Some(RelativeTime::Weeks(2))),
        ("1y", Some(RelativeTime::Years(1))),
        ("  10d  ", Some(RelativeTime::Days(10))),
        ("", None),
        ("   ", None),
        ("x", None),
        ("1", None),
        ("d", None),
        // We don't interpret +5d as "five days into the future"
        // because it doesn't make logical sense
        ("+5d", Some(RelativeTime::Days(5))),
        ("5q", None),
    ];

    for (input, expected) in cases {
        let got = parse_relative_time_literal(input);
        assert_eq!(got, *expected, "input: {:?}", input);
    }
}

#[test]
fn is_bits_unit_smartcase_behavior() {
    let cases: &[(&[u8], bool)] = &[
        (b"", false),
        (b"b", false),
        (b"kb", false),
        (b"mb", false),
        (b"MB", false),
        (b"Kb", true),
        (b"Mb", true),
        (b"Gb", true),
        (b"Tb", true),
        (b"KiB", false),
        (b"MiB", false),
        (b"Kib", true),
    ];

    for (unit, expected) in cases {
        let got = is_bits_unit(unit);
        assert_eq!(got, *expected, "unit: {:?}", std::str::from_utf8(unit));
    }
}

#[test]
fn parse_size_parses_raw_bytes_and_units() {
    let cases: &[(&str, Option<u64>)] = &[
        ("0", Some(0)),
        ("10", Some(10)),
        ("  10  ", Some(10)),
        ("10k", Some(10 * KIB)),
        ("10K", Some(10 * KIB)),
        ("10kb", Some(10 * KIB)),
        ("10KB", Some(10 * KIB)),
        ("10Ki", Some(10 * KIB)),
        ("10KiB", Some(10 * KIB)),
        ("1m", Some(1 * MIB)),
        ("1M", Some(1 * MIB)),
        ("1Mi", Some(1 * MIB)),
        ("1MiB", Some(1 * MIB)),
        ("2g", Some(2 * GIB)),
        ("2G", Some(2 * GIB)),
        ("2GiB", Some(2 * GIB)),
        ("3t", Some(3 * TIB)),
        ("3T", Some(3 * TIB)),
        ("3Ti", Some(3 * TIB)),
        ("3TiB", Some(3 * TIB)),
        ("", None),
        ("   ", None),
        ("abc", None),
        ("10x", None),
    ];

    for (input, expected) in cases {
        let got = parse_size(input);
        assert_eq!(got, *expected, "input: {:?}", input);
    }
}

#[test]
fn parse_size_handles_bits_via_smartcase() {
    let one_mib_bytes = MIB;
    let expected_one_megabit_bytes = one_mib_bytes / 8;

    let cases: &[(&str, Option<u64>)] = &[
        ("1Mb", Some(expected_one_megabit_bytes)),
        ("1mb", Some(1 * MIB)),
        ("1MB", Some(1 * MIB)),
        ("8Kb", Some(KIB)),
    ];

    for (input, expected) in cases {
        let got = parse_size(input);
        assert_eq!(got, *expected, "input: {:?}", input);
    }
}
