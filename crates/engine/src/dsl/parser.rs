use crate::dsl::ast::{LeafExpr, Query, QueryExpr, TextTerm};
use crate::dsl::lexer::{Token, TokenKind, lex};
use crate::dsl::predicates::parse_field_predicate;

#[derive(Debug, Clone)]
pub(crate) enum RawAtom<'a> {
    Field {
        field_name: &'a str,
        value_tokens: Vec<Token<'a>>,
    },
    Bare {
        tokens: Vec<Token<'a>>,
    },
}

struct Parser<'a> {
    tokens: &'a [Token<'a>],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token<'a>]) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> TokenKind {
        self.tokens
            .get(self.pos)
            .map(|t| t.kind)
            .unwrap_or(TokenKind::Eof)
    }

    fn advance(&mut self) -> Token<'a> {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or(Token {
            kind: TokenKind::Eof,
            lexeme: "",
            span: 0..0,
        });
        self.pos += 1;
        tok
    }

    /// Entry point for boolean expression parsing.
    fn parse_or_expr(&mut self) -> QueryExpr {
        let lhs = self.parse_and_expr();
        let mut ors = Vec::new();
        ors.push(lhs);

        while self.peek() == TokenKind::Or {
            self.advance();
            let rhs = self.parse_and_expr();
            ors.push(rhs);
        }

        if ors.len() == 1 {
            ors.pop().unwrap()
        } else {
            QueryExpr::Or(ors)
        }
    }

    fn parse_and_expr(&mut self) -> QueryExpr {
        let mut terms = Vec::new();
        let first = self.parse_not_expr();
        terms.push(first);

        loop {
            match self.peek() {
                TokenKind::Or | TokenKind::RParen | TokenKind::Eof => break,
                _ => {}
            }

            // Optional explicit AND.
            if self.peek() == TokenKind::And {
                self.advance();
            }

            let next = self.parse_not_expr();
            terms.push(next);
        }

        if terms.len() == 1 {
            terms.pop().unwrap()
        } else {
            QueryExpr::And(terms)
        }
    }

    fn parse_not_expr(&mut self) -> QueryExpr {
        let mut neg_count = 0;

        while self.peek() == TokenKind::Not {
            self.advance();
            neg_count += 1;
        }

        let mut expr = self.parse_primary();

        if neg_count % 2 == 1 {
            expr = QueryExpr::Not(Box::new(expr));
        }

        expr
    }

    fn parse_primary(&mut self) -> QueryExpr {
        match self.peek() {
            TokenKind::LParen => {
                self.advance(); // '('
                let expr = self.parse_or_expr();
                if self.peek() == TokenKind::RParen {
                    self.advance();
                }
                expr
            }
            TokenKind::Eof | TokenKind::RParen | TokenKind::Or | TokenKind::And => {
                // Degenerate positions (leading AND/OR, stray ')', etc.) are treated
                // as a neutral "true" term, which is the identity for AND.
                true_expr()
            }
            _ => {
                let atom = self.parse_raw_atom();
                QueryExpr::Leaf(resolve_atom(atom))
            }
        }
    }

    fn parse_raw_atom(&mut self) -> RawAtom<'a> {
        // Look for IDENT ':' pattern.
        let next_kind = self
            .tokens
            .get(self.pos + 1)
            .map(|t| t.kind)
            .unwrap_or(TokenKind::Eof);

        if self.peek() == TokenKind::Ident && next_kind == TokenKind::Colon {
            let field_tok = self.advance(); // IDENT
            self.advance(); // Colon

            // For field predicates, consume:
            // - Optional comparison operator (>, <, >=, <=, =)
            // - Exactly one value token (ident, number, or string)
            //
            // NOTE: multi-word values must be quoted (e.g. name:"foo bar").
            // Input like `name:foo bar` is parsed as `name:foo` plus a bare `bar`.
            let mut value_tokens = Vec::new();

            // Consume optional comparison operator
            if matches!(
                self.peek(),
                TokenKind::Gt | TokenKind::Lt | TokenKind::Gte | TokenKind::Lte | TokenKind::Eq
            ) {
                value_tokens.push(self.advance());
            }

            if matches!(
                self.peek(),
                TokenKind::Ident | TokenKind::Number | TokenKind::String
            ) {
                value_tokens.push(self.advance());
            }

            RawAtom::Field {
                field_name: field_tok.lexeme,
                value_tokens,
            }
        } else {
            // Bare atom: just this token.
            let tok = self.advance();
            RawAtom::Bare { tokens: vec![tok] }
        }
    }
}

/// Neutral boolean expression that always matches: the identity for AND.
fn true_expr() -> QueryExpr {
    QueryExpr::And(Vec::new())
}

/// Public entry point
pub fn parse_query(input: &str) -> Query {
    let tokens = lex(input);

    // Empty or whitespace-only input: treat as "match everything".
    if tokens.len() == 1 && tokens[0].kind == TokenKind::Eof {
        return Query { expr: true_expr() };
    }

    let mut parser = Parser::new(&tokens);
    let expr = parser.parse_or_expr();
    Query { expr }
}

/// Resolve a RawAtom into a typed leaf: predicate or text term.
fn resolve_atom(atom: RawAtom<'_>) -> LeafExpr {
    match atom {
        RawAtom::Field {
            field_name,
            value_tokens,
        } => {
            let field_name_lc = field_name.to_ascii_lowercase();
            let pred = parse_field_predicate(&field_name_lc, &value_tokens);

            match pred {
                Some(p) => LeafExpr::Predicate(p),
                None => LeafExpr::Text(text_from_field_atom(field_name, &value_tokens)),
            }
        }
        RawAtom::Bare { tokens } => LeafExpr::Text(text_from_tokens(&tokens)),
    }
}

pub(crate) fn text_from_tokens(tokens: &[Token<'_>]) -> TextTerm {
    if tokens.is_empty() {
        return TextTerm {
            text: String::new(),
            is_phrase: false,
            is_glob: false,
        };
    }

    // Join lexemes with spaces
    let mut text = String::new();
    for (i, t) in tokens.iter().enumerate() {
        if i > 0 {
            text.push(' ');
        }
        text.push_str(t.lexeme);
    }

    let first_kind = tokens[0].kind;
    TextTerm {
        is_phrase: matches!(first_kind, TokenKind::String),
        is_glob: text.contains('*') || text.contains('?'),
        text,
    }
}

fn text_from_field_atom(field_name: &str, value_tokens: &[Token<'_>]) -> TextTerm {
    let mut s = String::new();
    s.push_str(field_name);
    s.push(':');
    for (i, t) in value_tokens.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(t.lexeme);
    }

    TextTerm {
        is_phrase: false,
        is_glob: s.contains('*') || s.contains('?'),
        text: s,
    }
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;
