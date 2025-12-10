use super::parse_query;
use crate::dsl::ast::{CmpOp, Field, LeafExpr, QueryExpr, Value};

fn expr(input: &str) -> QueryExpr {
    parse_query(input).expr
}

fn text_leaf(expr: &QueryExpr) -> &str {
    match expr {
        QueryExpr::Leaf(LeafExpr::Text(term)) => term.text.as_str(),
        _ => panic!("expected text leaf, got {:?}", expr),
    }
}

fn is_phrase(expr: &QueryExpr) -> bool {
    match expr {
        QueryExpr::Leaf(LeafExpr::Text(term)) => term.is_phrase,
        _ => panic!("expected text leaf, got {:?}", expr),
    }
}

fn is_glob(expr: &QueryExpr) -> bool {
    match expr {
        QueryExpr::Leaf(LeafExpr::Text(term)) => term.is_glob,
        _ => panic!("expected text leaf, got {:?}", expr),
    }
}

fn predicate_leaf(expr: &QueryExpr) -> &crate::dsl::predicates::Predicate {
    match expr {
        QueryExpr::Leaf(LeafExpr::Predicate(p)) => p,
        _ => panic!("expected predicate leaf, got {:?}", expr),
    }
}

#[test]
fn empty_input_is_true_expr() {
    let q = expr("");
    match q {
        QueryExpr::And(children) => {
            assert!(children.is_empty(), "expected And([]), got {:?}", children)
        }
        _ => panic!("expected And([]) for empty input, got {:?}", q),
    }

    let q_ws = expr("   \t ");
    match q_ws {
        QueryExpr::And(children) => {
            assert!(children.is_empty(), "expected And([]), got {:?}", children)
        }
        _ => panic!("expected And([]) for whitespace input, got {:?}", q_ws),
    }
}

#[test]
fn single_bare_ident() {
    let q = expr("foo");
    assert_eq!(text_leaf(&q), "foo");
    assert!(!is_phrase(&q));
    assert!(!is_glob(&q));
}

#[test]
fn phrase_from_string_literal() {
    let q = expr(r#""hello world""#);
    assert_eq!(text_leaf(&q), "hello world");
    assert!(is_phrase(&q));
    assert!(!is_glob(&q));
}

#[test]
fn glob_detection_in_bare_term() {
    let q = expr("foo*bar?");
    assert_eq!(text_leaf(&q), "foo*bar?");
    assert!(!is_phrase(&q));
    assert!(is_glob(&q));
}

#[test]
fn glob_detection_in_field_fallback_text() {
    // Unknown field name: should fall back to TextTerm via text_from_field_atom.
    let q = expr("unknown:foo*");
    let text = text_leaf(&q);
    assert_eq!(text, "unknown:foo*");
    assert!(is_glob(&q));
}

#[test]
fn implicit_and_between_terms() {
    let q = expr("foo bar baz");
    match q {
        QueryExpr::And(children) => {
            assert_eq!(children.len(), 3);
            assert_eq!(text_leaf(&children[0]), "foo");
            assert_eq!(text_leaf(&children[1]), "bar");
            assert_eq!(text_leaf(&children[2]), "baz");
        }
        _ => panic!("expected And([...]) for implicit AND, got {:?}", q),
    }
}

#[test]
fn explicit_and_is_equivalent_to_implicit_and() {
    let implicit = expr("foo bar");
    let explicit = expr("foo AND bar");

    fn texts(e: &QueryExpr) -> Vec<String> {
        match e {
            QueryExpr::And(children) => children.iter().map(|c| text_leaf(c).to_string()).collect(),
            _ => panic!("expected And([...]), got {:?}", e),
        }
    }

    assert_eq!(texts(&implicit), vec!["foo".to_string(), "bar".to_string()]);
    assert_eq!(texts(&explicit), vec!["foo".to_string(), "bar".to_string()]);
}

#[test]
fn or_is_lowest_precedence() {
    // foo AND bar OR baz  => (foo AND bar) OR baz
    let q = expr("foo AND bar OR baz");
    match q {
        QueryExpr::Or(ors) => {
            assert_eq!(ors.len(), 2);
            match &ors[0] {
                QueryExpr::And(children) => {
                    assert_eq!(children.len(), 2);
                    assert_eq!(text_leaf(&children[0]), "foo");
                    assert_eq!(text_leaf(&children[1]), "bar");
                }
                _ => panic!(
                    "expected first OR branch to be And([...]), got {:?}",
                    ors[0]
                ),
            }
            assert_eq!(text_leaf(&ors[1]), "baz");
        }
        _ => panic!("expected Or([...]) for 'foo AND bar OR baz', got {:?}", q),
    }
}

#[test]
fn and_binds_tighter_than_or() {
    // foo OR bar AND baz  => foo OR (bar AND baz)
    let q = expr("foo OR bar AND baz");
    match q {
        QueryExpr::Or(ors) => {
            assert_eq!(ors.len(), 2);
            assert_eq!(text_leaf(&ors[0]), "foo");
            match &ors[1] {
                QueryExpr::And(children) => {
                    assert_eq!(children.len(), 2);
                    assert_eq!(text_leaf(&children[0]), "bar");
                    assert_eq!(text_leaf(&children[1]), "baz");
                }
                _ => panic!(
                    "expected second OR branch to be And([...]), got {:?}",
                    ors[1]
                ),
            }
        }
        _ => panic!("expected Or([...]) for 'foo OR bar AND baz', got {:?}", q),
    }
}

#[test]
fn not_expression_and_double_not() {
    let q = expr("NOT foo");
    match q {
        QueryExpr::Not(inner) => {
            assert_eq!(text_leaf(&inner), "foo");
        }
        _ => panic!("expected Not(Leaf), got {:?}", q),
    }

    let q2 = expr("NOT NOT foo");
    // Double NOT should cancel; we just check we don't get a nested Not(Not(_)).
    match q2 {
        QueryExpr::Leaf(LeafExpr::Text(term)) => assert_eq!(term.text, "foo"),
        QueryExpr::Not(inner) => panic!("expected double NOT to cancel, got {:?}", inner),
        _ => panic!("unexpected shape for 'NOT NOT foo': {:?}", q2),
    }
}

#[test]
fn parentheses_affect_precedence() {
    // (foo OR bar) AND baz
    let q = expr("(foo OR bar) AND baz");
    match q {
        QueryExpr::And(ands) => {
            assert_eq!(ands.len(), 2);
            match &ands[0] {
                QueryExpr::Or(ors) => {
                    assert_eq!(ors.len(), 2);
                    assert_eq!(text_leaf(&ors[0]), "foo");
                    assert_eq!(text_leaf(&ors[1]), "bar");
                }
                _ => panic!(
                    "expected first AND child to be Or([...]), got {:?}",
                    ands[0]
                ),
            }
            assert_eq!(text_leaf(&ands[1]), "baz");
        }
        _ => panic!(
            "expected And([...]) for '(foo OR bar) AND baz', got {:?}",
            q
        ),
    }
}

#[test]
fn leading_and_or_are_treated_as_true_identity() {
    // "AND foo" -> And([True, foo]) where True is And([])
    let q = expr("AND foo");
    match q {
        QueryExpr::And(children) => {
            assert_eq!(children.len(), 2);
            match &children[0] {
                QueryExpr::And(inner) => {
                    assert!(inner.is_empty(), "expected True expr as first child")
                }
                _ => panic!(
                    "expected first child to be True expr, got {:?}",
                    children[0]
                ),
            }
            assert_eq!(text_leaf(&children[1]), "foo");
        }
        _ => panic!("expected And([...]) for 'AND foo', got {:?}", q),
    }
}

#[test]
fn unknown_field_falls_back_to_text() {
    let q = expr("xyz:foo");
    let text = text_leaf(&q);
    assert_eq!(text, "xyz:foo");
    assert!(!is_phrase(&q));
}

#[test]
fn ext_field_parses_to_predicate() {
    let q = expr("ext:pdf");
    let p = predicate_leaf(&q);
    assert_eq!(p.field, Field::Ext);
    assert_eq!(p.op, CmpOp::Eq);
    match &p.value {
        Value::Str(s) => assert_eq!(s, "pdf"),
        other => panic!("expected Value::Str(\"pdf\"), got {:?}", other),
    }
}

#[test]
fn size_field_with_gt_operator_parses_to_predicate() {
    let q = expr("size:>10");
    let p = predicate_leaf(&q);
    assert_eq!(p.field, Field::Size);
    assert_eq!(p.op, CmpOp::Gt);
    // We deliberately do not over-specify the numeric conversion semantics;
    // it's enough that a numeric Value is produced.
    match &p.value {
        Value::SizeBytes(v) => assert!(*v > 0, "expected positive size, got {}", v),
        other => panic!("expected Value::SizeBytes(_), got {:?}", other),
    }
}
