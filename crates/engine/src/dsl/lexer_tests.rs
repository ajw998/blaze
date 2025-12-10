use super::{Token, TokenKind, lex};

fn kinds_lexemes(input: &str) -> Vec<(TokenKind, &str)> {
    lex(input).into_iter().map(|t| (t.kind, t.lexeme)).collect()
}

#[test]
fn basic_ident_and_number() {
    use TokenKind::*;
    assert_eq!(
        kinds_lexemes("foo 123"),
        vec![(Ident, "foo"), (Number, "123"), (Eof, "")]
    );
}

#[test]
fn keywords_are_case_insensitive() {
    use TokenKind::*;
    assert_eq!(
        kinds_lexemes("and AND Or not NOT"),
        vec![
            (And, "and"),
            (And, "AND"),
            (Or, "Or"),
            (Not, "not"),
            (Not, "NOT"),
            (Eof, ""),
        ]
    );
}

#[test]
fn operators_and_punctuation() {
    use TokenKind::*;
    assert_eq!(
        kinds_lexemes("ext:pdf (a>1 AND b>=2) c<3 OR d<=4 e=5"),
        vec![
            (Ident, "ext"),
            (Colon, ":"),
            (Ident, "pdf"),
            (LParen, "("),
            (Ident, "a"),
            (Gt, ">"),
            (Number, "1"),
            (And, "AND"),
            (Ident, "b"),
            (Gte, ">="),
            (Number, "2"),
            (RParen, ")"),
            (Ident, "c"),
            (Lt, "<"),
            (Number, "3"),
            (Or, "OR"),
            (Ident, "d"),
            (Lte, "<="),
            (Number, "4"),
            (Ident, "e"),
            (Eq, "="),
            (Number, "5"),
            (Eof, ""),
        ]
    );
}

#[test]
fn string_literals_and_spans() {
    use TokenKind::*;
    let tokens = lex(r#""hello world""#);
    assert_eq!(tokens.len(), 2);

    let t0: &Token<'_> = &tokens[0];
    assert_eq!(t0.kind, String);
    assert_eq!(t0.lexeme, "hello world");
    assert_eq!(t0.span, 0..13); // " + 11 chars + "

    let eof = &tokens[1];
    assert_eq!(eof.kind, Eof);
    assert_eq!(eof.span, 13..13);
}

#[test]
fn unterminated_string_consumes_to_end() {
    use TokenKind::*;
    let tokens = lex(r#""unterminated"#);
    assert_eq!(tokens[0].kind, String);
    assert_eq!(tokens[0].lexeme, "unterminated");
    assert_eq!(tokens[1].kind, Eof);
}

#[test]
fn bar_variants_behave_as_designed() {
    use TokenKind::*;
    // "a||b" is a single ident, "a || b" uses logical OR, single '|' stays in idents.
    assert_eq!(
        kinds_lexemes("a||b a || b a|b |"),
        vec![
            (Ident, "a||b"),
            (Ident, "a"),
            (Or, "||"),
            (Ident, "b"),
            (Ident, "a|b"),
            (Ident, "|"),
            (Eof, ""),
        ]
    );
}

#[test]
fn dots_and_minus_stay_in_idents_not_numbers() {
    use TokenKind::*;
    assert_eq!(
        kinds_lexemes("1.5 -3 file-name.txt"),
        vec![
            (Ident, "1.5"),
            (Ident, "-3"),
            (Ident, "file-name.txt"),
            (Eof, ""),
        ]
    );
}
