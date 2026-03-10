use crate::syntax::span::{FileId, Span};
use crate::syntax::tokens::{Token, TokenKind};

pub type LexResult<T> = Result<T, LexError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub kind: LexErrorKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexErrorKind {
    UnterminatedString,
    InvalidEscape(char),
    InvalidNumber,
    InvalidHexLiteral,
    UnexpectedChar(char),
    InvalidUtf8,
}

impl LexError {
    fn new(kind: LexErrorKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// Lexer state for string interpolation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StringState {
    NotInString,
    InString { brace_depth: u32 },
}

pub struct Lexer {
    source: Vec<char>,
    file_id: FileId,
    pos: usize,
    string_state: StringState,
}

impl Lexer {
    pub fn new(source: &str, file_id: FileId) -> Self {
        Self {
            source: source.chars().collect(),
            file_id,
            pos: 0,
            string_state: StringState::NotInString,
        }
    }

    /// Lex the entire source into tokens
    pub fn lex(source: &str, file_id: FileId) -> LexResult<Vec<Token>> {
        let mut lexer = Lexer::new(source, file_id);
        let mut tokens = Vec::new();

        loop {
            let token = lexer.next_token()?;
            let is_eof = token.kind == TokenKind::Eof;
            tokens.push(token);
            if is_eof {
                break;
            }
        }

        Ok(tokens)
    }

    fn next_token(&mut self) -> LexResult<Token> {
        // Handle string interpolation state
        if let StringState::InString { brace_depth } = self.string_state {
            return self.lex_string_continuation(brace_depth);
        }

        let saw_newline = self.skip_whitespace_and_comments();

        let start = self.pos;

        if self.is_eof() {
            return Ok(Token::eof(self.file_id, start as u32));
        }

        let ch = self.peek();

        let mut token = match ch {
            // String literals
            '"' => self.lex_string()?,

            // Numbers
            '0'..='9' => self.lex_number()?,

            // Identifiers and keywords
            'a'..='z' | 'A'..='Z' | '_' => self.lex_ident_or_keyword(),

            // Operators and punctuation
            '+' => self.lex_single(TokenKind::Plus),
            '-' => self.lex_single(TokenKind::Minus),
            '*' => self.lex_single(TokenKind::Star),
            '/' => self.lex_single(TokenKind::Slash),
            '%' => self.lex_single(TokenKind::Percent),
            '=' => self.lex_char_or_double(
                TokenKind::Eq,
                '=',
                TokenKind::EqEq,
                '>',
                TokenKind::FatArrow,
            ),
            '!' => self.lex_char_or_double(
                TokenKind::Bang,
                '=',
                TokenKind::BangEq,
                '\0',
                TokenKind::Error,
            ),
            '<' => {
                self.lex_char_or_double(TokenKind::Lt, '=', TokenKind::LtEq, '\0', TokenKind::Error)
            }
            '>' => {
                self.lex_char_or_double(TokenKind::Gt, '=', TokenKind::GtEq, '\0', TokenKind::Error)
            }
            ':' => self.lex_char_or_double(
                TokenKind::Colon,
                '=',
                TokenKind::ColonEq,
                '\0',
                TokenKind::Error,
            ),

            '&' => self.lex_single(TokenKind::Amp),
            '|' => self.lex_single(TokenKind::Pipe),
            '^' => self.lex_single(TokenKind::Caret),
            '~' => self.lex_single(TokenKind::Tilde),

            '(' => self.lex_single(TokenKind::LParen),
            ')' => self.lex_single(TokenKind::RParen),
            '{' => self.lex_single(TokenKind::LBrace),
            '}' => self.lex_single(TokenKind::RBrace),
            '[' => self.lex_single(TokenKind::LBracket),
            ']' => self.lex_single(TokenKind::RBracket),
            ',' => self.lex_single(TokenKind::Comma),
            '.' => self.lex_single(TokenKind::Dot),
            '?' => self.lex_single(TokenKind::Question),
            ';' => self.lex_single(TokenKind::Semi),
            '@' => self.lex_single(TokenKind::At),

            _ => {
                self.advance();
                let span = Span::new(self.file_id, start as u32, self.pos as u32);
                return Err(LexError::new(LexErrorKind::UnexpectedChar(ch), span));
            }
        };

        token.preceded_by_newline = saw_newline;
        Ok(token)
    }

    fn lex_single(&mut self, kind: TokenKind) -> Token {
        let start = self.pos;
        let ch = self.advance();
        let span = Span::new(self.file_id, start as u32, self.pos as u32);
        Token::new(kind, span, ch.to_string())
    }

    fn lex_char_or_double(
        &mut self,
        single: TokenKind,
        next1: char,
        double1: TokenKind,
        next2: char,
        double2: TokenKind,
    ) -> Token {
        let start = self.pos;
        let ch = self.advance();
        let next = self.peek();

        if next == next1 {
            self.advance();
            let span = Span::new(self.file_id, start as u32, self.pos as u32);
            Token::new(double1, span, format!("{}{}", ch, next1))
        } else if next2 != '\0' && next == next2 {
            self.advance();
            let span = Span::new(self.file_id, start as u32, self.pos as u32);
            Token::new(double2, span, format!("{}{}", ch, next2))
        } else {
            let span = Span::new(self.file_id, start as u32, self.pos as u32);
            Token::new(single, span, ch.to_string())
        }
    }

    fn lex_ident_or_keyword(&mut self) -> Token {
        let start = self.pos;

        while !self.is_eof() && self.is_ident_continue(self.peek()) {
            self.advance();
        }

        let span = Span::new(self.file_id, start as u32, self.pos as u32);
        let text: String = self.source[start..self.pos].iter().collect();

        // Check if it's a keyword
        let kind = TokenKind::from_keyword(&text).unwrap_or(TokenKind::Ident);

        Token::new(kind, span, text)
    }

    fn lex_number(&mut self) -> LexResult<Token> {
        let start = self.pos;

        // Check for hex literal: 0x...
        if self.peek() == '0' && self.peek_ahead(1) == 'x' {
            self.advance(); // consume '0'
            self.advance(); // consume 'x'

            // Must have at least one hex digit
            if self.is_eof() || !self.peek().is_ascii_hexdigit() {
                let span = Span::new(self.file_id, start as u32, self.pos as u32);
                return Err(LexError::new(LexErrorKind::InvalidHexLiteral, span));
            }

            while !self.is_eof() && self.peek().is_ascii_hexdigit() {
                self.advance();
            }

            let span = Span::new(self.file_id, start as u32, self.pos as u32);
            let text: String = self.source[start..self.pos].iter().collect();
            return Ok(Token::new(TokenKind::IntLit, span, text));
        }

        let mut is_float = false;

        // Read digits
        while !self.is_eof() && self.peek().is_ascii_digit() {
            self.advance();
        }

        // Check for decimal point
        if !self.is_eof() && self.peek() == '.' && self.peek_ahead(1).is_ascii_digit() {
            is_float = true;
            self.advance(); // consume '.'

            while !self.is_eof() && self.peek().is_ascii_digit() {
                self.advance();
            }
        }

        let span = Span::new(self.file_id, start as u32, self.pos as u32);
        let text: String = self.source[start..self.pos].iter().collect();

        let kind = if is_float {
            TokenKind::FloatLit
        } else {
            TokenKind::IntLit
        };

        Ok(Token::new(kind, span, text))
    }

    fn lex_string(&mut self) -> LexResult<Token> {
        let start = self.pos;
        self.advance(); // consume opening "

        let mut value = String::new();
        let mut has_interpolation = false;

        while !self.is_eof() && self.peek() != '"' {
            if self.peek() == '\\' {
                self.advance();
                if self.is_eof() {
                    let span = Span::new(self.file_id, start as u32, self.pos as u32);
                    return Err(LexError::new(LexErrorKind::UnterminatedString, span));
                }
                let escaped = self.escape_char()?;
                value.push(escaped);
            } else if self.peek() == '$' && self.peek_ahead(1) == '{' {
                // String interpolation
                has_interpolation = true;
                break;
            } else {
                value.push(self.advance());
            }
        }

        if has_interpolation {
            // This is a string with interpolation
            self.advance(); // consume $
            self.advance(); // consume {
            self.string_state = StringState::InString { brace_depth: 1 };

            let span = Span::new(self.file_id, start as u32, self.pos as u32);
            Ok(Token::new(TokenKind::StringStart, span, value))
        } else {
            // Simple string without interpolation
            if self.is_eof() {
                let span = Span::new(self.file_id, start as u32, self.pos as u32);
                return Err(LexError::new(LexErrorKind::UnterminatedString, span));
            }

            self.advance(); // consume closing "
            let span = Span::new(self.file_id, start as u32, self.pos as u32);
            Ok(Token::new(TokenKind::StringLit, span, value))
        }
    }

    fn lex_string_continuation(&mut self, brace_depth: u32) -> LexResult<Token> {
        // We're inside a string interpolation, lexing tokens until we hit a closing }

        // First, check if we're at a brace
        if self.peek() == '{' {
            let new_depth = brace_depth + 1;
            self.string_state = StringState::InString {
                brace_depth: new_depth,
            };
            return Ok(self.lex_single(TokenKind::LBrace));
        }

        if self.peek() == '}' {
            // Guard against underflow
            if brace_depth == 0 {
                let start = self.pos;
                self.advance();
                let span = Span::new(self.file_id, start as u32, self.pos as u32);
                return Err(LexError::new(LexErrorKind::UnexpectedChar('}'), span));
            }

            let new_depth = brace_depth - 1;

            if new_depth == 0 {
                // End of interpolation, resume string
                let start = self.pos;
                self.advance(); // consume }

                let mut value = String::new();
                let mut has_more_interpolation = false;

                while !self.is_eof() && self.peek() != '"' {
                    if self.peek() == '\\' {
                        self.advance();
                        if self.is_eof() {
                            let span = Span::new(self.file_id, start as u32, self.pos as u32);
                            return Err(LexError::new(LexErrorKind::UnterminatedString, span));
                        }
                        let escaped = self.escape_char()?;
                        value.push(escaped);
                    } else if self.peek() == '$' && self.peek_ahead(1) == '{' {
                        has_more_interpolation = true;
                        break;
                    } else {
                        value.push(self.advance());
                    }
                }

                if has_more_interpolation {
                    self.advance(); // consume $
                    self.advance(); // consume {
                    self.string_state = StringState::InString { brace_depth: 1 };

                    let span = Span::new(self.file_id, start as u32, self.pos as u32);
                    Ok(Token::new(TokenKind::StringContinue, span, value))
                } else {
                    if self.is_eof() {
                        let span = Span::new(self.file_id, start as u32, self.pos as u32);
                        return Err(LexError::new(LexErrorKind::UnterminatedString, span));
                    }

                    self.advance(); // consume closing "
                    self.string_state = StringState::NotInString;

                    let span = Span::new(self.file_id, start as u32, self.pos as u32);
                    Ok(Token::new(TokenKind::StringEnd, span, value))
                }
            } else {
                self.string_state = StringState::InString {
                    brace_depth: new_depth,
                };
                Ok(self.lex_single(TokenKind::RBrace))
            }
        } else {
            // Regular token inside interpolation - temporarily exit string state
            self.string_state = StringState::NotInString;
            let token = self.next_token()?;
            self.string_state = StringState::InString { brace_depth };
            Ok(token)
        }
    }

    fn escape_char(&mut self) -> LexResult<char> {
        let start = self.pos - 1; // -1 for the backslash
        let ch = self.advance();

        match ch {
            'n' => Ok('\n'),
            't' => Ok('\t'),
            'r' => Ok('\r'),
            '"' => Ok('"'),
            '\\' => Ok('\\'),
            '$' => Ok('$'),
            _ => {
                let span = Span::new(self.file_id, start as u32, self.pos as u32);
                Err(LexError::new(LexErrorKind::InvalidEscape(ch), span))
            }
        }
    }

    /// Skips whitespace and line comments; returns true if at least one newline was consumed.
    fn skip_whitespace_and_comments(&mut self) -> bool {
        let mut saw_newline = false;
        while !self.is_eof() {
            let ch = self.peek();

            if ch == '\n' {
                saw_newline = true;
                self.advance();
            } else if ch.is_whitespace() {
                self.advance();
            } else if ch == '/' && self.peek_ahead(1) == '/' {
                // Skip single-line comment (the newline that ends it will be caught next iter)
                while !self.is_eof() && self.peek() != '\n' {
                    self.advance();
                }
            } else {
                break;
            }
        }
        saw_newline
    }

    fn is_ident_continue(&self, ch: char) -> bool {
        ch.is_alphanumeric() || ch == '_'
    }

    fn peek(&self) -> char {
        self.source.get(self.pos).copied().unwrap_or('\0')
    }

    fn peek_ahead(&self, n: usize) -> char {
        self.source.get(self.pos + n).copied().unwrap_or('\0')
    }

    fn advance(&mut self) -> char {
        let ch = self.peek();
        self.pos += 1;
        ch
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.source.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_simple(source: &str) -> Vec<Token> {
        Lexer::lex(source, FileId(0)).unwrap()
    }

    #[test]
    fn test_lex_keywords() {
        let tokens = lex_simple("fn if else type");
        assert_eq!(tokens.len(), 5); // 4 keywords + EOF
        assert_eq!(tokens[0].kind, TokenKind::Fn);
        assert_eq!(tokens[1].kind, TokenKind::If);
        assert_eq!(tokens[2].kind, TokenKind::Else);
        assert_eq!(tokens[3].kind, TokenKind::Type);
    }

    #[test]
    fn test_lex_numbers() {
        let tokens = lex_simple("42 3.14 0");
        assert_eq!(tokens[0].kind, TokenKind::IntLit);
        assert_eq!(tokens[0].text, "42");
        assert_eq!(tokens[1].kind, TokenKind::FloatLit);
        assert_eq!(tokens[1].text, "3.14");
        assert_eq!(tokens[2].kind, TokenKind::IntLit);
        assert_eq!(tokens[2].text, "0");
    }

    #[test]
    fn test_lex_string_simple() {
        let tokens = lex_simple(r#""hello world""#);
        assert_eq!(tokens[0].kind, TokenKind::StringLit);
        assert_eq!(tokens[0].text, "hello world");
    }

    #[test]
    fn test_lex_string_escapes() {
        let tokens = lex_simple(r#""hello\nworld\t""#);
        assert_eq!(tokens[0].kind, TokenKind::StringLit);
        assert_eq!(tokens[0].text, "hello\nworld\t");
    }

    #[test]
    fn test_lex_string_interpolation() {
        let tokens = lex_simple(r#""x=${x}""#);
        // Should produce: STRING_START("x="), IDENT("x"), STRING_END(""), EOF
        // Note: The } is consumed internally when transitioning back to string mode
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].kind, TokenKind::StringStart);
        assert_eq!(tokens[0].text, "x=");
        assert_eq!(tokens[1].kind, TokenKind::Ident);
        assert_eq!(tokens[1].text, "x");
        assert_eq!(tokens[2].kind, TokenKind::StringEnd);
        assert_eq!(tokens[2].text, "");
        assert_eq!(tokens[3].kind, TokenKind::Eof);
    }

    #[test]
    fn test_lex_operators() {
        let tokens = lex_simple("+ - * / % == != < <= > >= := =>");
        assert_eq!(tokens[0].kind, TokenKind::Plus);
        assert_eq!(tokens[1].kind, TokenKind::Minus);
        assert_eq!(tokens[2].kind, TokenKind::Star);
        assert_eq!(tokens[3].kind, TokenKind::Slash);
        assert_eq!(tokens[4].kind, TokenKind::Percent);
        assert_eq!(tokens[5].kind, TokenKind::EqEq);
        assert_eq!(tokens[6].kind, TokenKind::BangEq);
        assert_eq!(tokens[7].kind, TokenKind::Lt);
        assert_eq!(tokens[8].kind, TokenKind::LtEq);
        assert_eq!(tokens[9].kind, TokenKind::Gt);
        assert_eq!(tokens[10].kind, TokenKind::GtEq);
        assert_eq!(tokens[11].kind, TokenKind::ColonEq);
        assert_eq!(tokens[12].kind, TokenKind::FatArrow);
    }

    #[test]
    fn test_lex_comments() {
        let tokens = lex_simple("x // comment\ny");
        assert_eq!(tokens.len(), 3); // x, y, EOF
        assert_eq!(tokens[0].text, "x");
        assert_eq!(tokens[1].text, "y");
    }

    #[test]
    fn test_lex_mixed() {
        let tokens = lex_simple("fn add(a: Int, b: Int) Int { a + b }");
        assert_eq!(tokens[0].kind, TokenKind::Fn);
        assert_eq!(tokens[1].kind, TokenKind::Ident);
        assert_eq!(tokens[1].text, "add");
        assert_eq!(tokens[2].kind, TokenKind::LParen);
    }

    #[test]
    fn test_lex_hex_literals() {
        let tokens = lex_simple("0xFF 0x0 0xdeadbeef 0x10");
        assert_eq!(tokens[0].kind, TokenKind::IntLit);
        assert_eq!(tokens[0].text, "0xFF");
        assert_eq!(tokens[1].kind, TokenKind::IntLit);
        assert_eq!(tokens[1].text, "0x0");
        assert_eq!(tokens[2].kind, TokenKind::IntLit);
        assert_eq!(tokens[2].text, "0xdeadbeef");
        assert_eq!(tokens[3].kind, TokenKind::IntLit);
        assert_eq!(tokens[3].text, "0x10");
    }

    #[test]
    fn test_lex_hex_no_digits() {
        let result = Lexer::lex("0x", FileId(0));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, LexErrorKind::InvalidHexLiteral);
    }

    #[test]
    fn test_lex_hex_invalid_digit() {
        // 0xG should lex as 0x (error) — G is not a hex digit
        let result = Lexer::lex("0xG", FileId(0));
        assert!(result.is_err());
    }
}
