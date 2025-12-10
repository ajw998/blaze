use std::{iter::Peekable, ops::Range, str::CharIndices};
// TODO: We need to consider how to handle cases where
// the file name like this_and_that.pdf, this_or_that.pdf

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TokenKind {
    // Identifiers, usually free texts
    // Examples: invoice, ext, /Users
    Ident,
    Number,
    String,
    Colon,
    LParen,
    RParen,
    And,
    Or,
    Not,
    // Greater than
    Gt,
    // Greater than or equal
    Gte,
    // Less than
    Lt,
    // Less than or equal
    Lte,
    // Equal
    Eq,
    Eof,
}

/// Single token with lexeme and span
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: TokenKind,
    pub lexeme: &'a str,
    pub span: Range<usize>,
}

pub struct Lexer<'a> {
    input: &'a str,
    chars: Peekable<CharIndices<'a>>,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            chars: input.char_indices().peekable(),
        }
    }

    fn advance_until(&mut self, end: usize) {
        while let Some(&(i, _)) = self.chars.peek() {
            if i >= end {
                break;
            }
            self.chars.next();
        }
    }

    /// Scan an identifier or number starting at `start` with `first_char`.
    fn scan_word_or_number(&mut self, start: usize, first_char: char) -> (TokenKind, usize) {
        let mut end = start + first_char.len_utf8();
        let mut all_ascii_digits = first_char.is_ascii_digit();

        // Consume until we hit a delimiter
        while let Some(&(i, c)) = self.chars.peek() {
            if is_delimiter(c) {
                break;
            }
            all_ascii_digits &= c.is_ascii_digit();
            end = i + c.len_utf8();
            self.chars.next();
        }

        let lexeme = &self.input[start..end];

        let kind = if all_ascii_digits {
            TokenKind::Number
        } else {
            classify_keyword(lexeme)
        };

        (kind, end)
    }

    /// Return the next token from the input.
    pub fn next_token(&mut self) -> Token<'a> {
        loop {
            let (start, c) = match self.chars.next() {
                Some(pair) => pair,
                None => {
                    let len = self.input.len();
                    return Token {
                        kind: TokenKind::Eof,
                        lexeme: "",
                        span: len..len,
                    };
                }
            };

            // Skip whitespace.
            if c.is_whitespace() {
                continue;
            }

            match c {
                '(' | ')' | ':' | '=' => {
                    let kind = match c {
                        '(' => TokenKind::LParen,
                        ')' => TokenKind::RParen,
                        ':' => TokenKind::Colon,
                        '=' => TokenKind::Eq,
                        _ => unreachable!(),
                    };
                    // All of these are ASCII single-byte characters.
                    let end = start + 1;
                    return Token {
                        kind,
                        lexeme: &self.input[start..end],
                        span: start..end,
                    };
                }
                '>' => {
                    let mut end = start + 1;
                    let mut kind = TokenKind::Gt;
                    if let Some(&(_, '=')) = self.chars.peek() {
                        self.chars.next();
                        end += 1; // '=' is ASCII
                        kind = TokenKind::Gte;
                    }
                    return Token {
                        kind,
                        lexeme: &self.input[start..end],
                        span: start..end,
                    };
                }
                '<' => {
                    let mut end = start + 1;
                    let mut kind = TokenKind::Lt;
                    if let Some(&(_, '=')) = self.chars.peek() {
                        self.chars.next();
                        end += 1; // '=' is ASCII
                        kind = TokenKind::Lte;
                    }
                    return Token {
                        kind,
                        lexeme: &self.input[start..end],
                        span: start..end,
                    };
                }
                '"' => {
                    // NOTE: No escaping: the next literal `"` terminates the string.
                    let content_start = start + 1;
                    let remainder = &self.input[content_start..];
                    if let Some(rel_end) = remainder.find('"') {
                        let content_end = content_start + rel_end;
                        let end = content_end + 1;
                        self.advance_until(end);
                        return Token {
                            kind: TokenKind::String,
                            lexeme: &self.input[content_start..content_end],
                            span: start..end,
                        };
                    } else {
                        let end = self.input.len();
                        self.advance_until(end);
                        return Token {
                            kind: TokenKind::String,
                            lexeme: &self.input[content_start..end],
                            span: start..end,
                        };
                    }
                }
                '|' => {
                    // Treat "||" as OR, single '|' as part of an identifier.
                    if let Some(&(_, '|')) = self.chars.peek() {
                        self.chars.next();
                        let end = start + 2;
                        return Token {
                            kind: TokenKind::Or,
                            lexeme: &self.input[start..end],
                            span: start..end,
                        };
                    } else {
                        let (kind, end) = self.scan_word_or_number(start, c);
                        return Token {
                            kind,
                            lexeme: &self.input[start..end],
                            span: start..end,
                        };
                    }
                }
                _ => {
                    // Identifier or number.
                    let (kind, end) = self.scan_word_or_number(start, c);
                    return Token {
                        kind,
                        lexeme: &self.input[start..end],
                        span: start..end,
                    };
                }
            }
        }
    }
}

// Notes
// "1.5" and "-3" are lexed as identifiers, not numbers.
// Path-like strings (e.g. "/Users/foo-bar") stay as single identifiers.
#[inline]
fn is_delimiter(c: char) -> bool {
    c.is_whitespace() || matches!(c, '(' | ')' | ':' | '>' | '<' | '=' | '"')
}

#[inline]
fn classify_keyword(lexeme: &str) -> TokenKind {
    match lexeme.len() {
        2 if lexeme.eq_ignore_ascii_case("or") => TokenKind::Or,
        3 if lexeme.eq_ignore_ascii_case("and") => TokenKind::And,
        3 if lexeme.eq_ignore_ascii_case("not") => TokenKind::Not,
        _ => TokenKind::Ident,
    }
}

pub fn lex(input: &str) -> Vec<Token<'_>> {
    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::with_capacity(16);

    loop {
        let token = lexer.next_token();
        let is_eof = token.kind == TokenKind::Eof;
        tokens.push(token);
        if is_eof {
            break;
        }
    }

    tokens
}

#[cfg(test)]
#[path = "lexer_tests.rs"]
mod tests;
