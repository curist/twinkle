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
    InvalidHexEscape,
    InvalidUnicodeEscape,
    InvalidNumber,
    InvalidHexLiteral,
    UnterminatedChar,
    InvalidCharLiteral(&'static str),
    UnexpectedChar(char),
    InvalidUtf8,
}

impl LexError {
    fn new(kind: LexErrorKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// Which surface string an open interpolation belongs to, so the scan resuming
/// after `}` picks the matching segment rules (escapes processed or raw).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StringKind {
    Cooked,
    Raw,
}

pub struct Lexer {
    source: Vec<char>,
    file_id: FileId,
    pos: usize,
    /// Byte offset corresponding to `pos`, used for span creation.
    byte_pos: usize,
    /// Stack of active `${...}` interpolation contexts: `(brace_depth, host)`.
    interpolation_stack: Vec<(u32, StringKind)>,
}

impl Lexer {
    pub fn new(source: &str, file_id: FileId) -> Self {
        Self {
            source: source.chars().collect(),
            file_id,
            pos: 0,
            byte_pos: 0,
            interpolation_stack: Vec::new(),
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
        if !self.interpolation_stack.is_empty() {
            return self.lex_string_continuation();
        }
        self.lex_regular_token()
    }

    fn lex_regular_token(&mut self) -> LexResult<Token> {
        let saw_newline = self.skip_whitespace_and_comments();

        let start = self.byte_pos;

        if self.is_eof() {
            return Ok(Token::eof(self.file_id, start as u32));
        }

        let ch = self.peek();

        let mut token = match ch {
            // String literals
            '"' => self.lex_string()?,

            // Raw single-line string: `r` immediately followed by `"`. Any other
            // `r` is an ordinary identifier (handled by the alphanumeric arm).
            'r' if self.peek_ahead(1) == '"' => self.lex_raw_string()?,

            // Character literals (lex to an integer code-point token)
            '\'' => self.lex_char()?,

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
            '.' => self.lex_char_or_double(
                TokenKind::Dot,
                '.',
                TokenKind::DotDot,
                '\0',
                TokenKind::Error,
            ),
            '?' => self.lex_single(TokenKind::Question),
            ';' => self.lex_single(TokenKind::Semi),
            '@' => self.lex_single(TokenKind::At),

            _ => {
                self.advance();
                let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                return Err(LexError::new(LexErrorKind::UnexpectedChar(ch), span));
            }
        };

        token.preceded_by_newline = saw_newline;
        Ok(token)
    }

    fn lex_single(&mut self, kind: TokenKind) -> Token {
        let start = self.byte_pos;
        let ch = self.advance();
        let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
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
        let start = self.byte_pos;
        let ch = self.advance();
        let next = self.peek();

        if next == next1 {
            self.advance();
            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            Token::new(double1, span, format!("{}{}", ch, next1))
        } else if next2 != '\0' && next == next2 {
            self.advance();
            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            Token::new(double2, span, format!("{}{}", ch, next2))
        } else {
            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            Token::new(single, span, ch.to_string())
        }
    }

    fn lex_ident_or_keyword(&mut self) -> Token {
        let start = self.byte_pos;
        let char_start = self.pos;

        while !self.is_eof() && self.is_ident_continue(self.peek()) {
            self.advance();
        }

        let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
        let text: String = self.source[char_start..self.pos].iter().collect();

        // Check if it's a keyword
        let kind = TokenKind::from_keyword(&text).unwrap_or(TokenKind::Ident);

        Token::new(kind, span, text)
    }

    fn lex_number(&mut self) -> LexResult<Token> {
        let start = self.byte_pos;
        let char_start = self.pos;

        // Check for hex literal: 0x...
        if self.peek() == '0' && self.peek_ahead(1) == 'x' {
            self.advance(); // consume '0'
            self.advance(); // consume 'x'

            // Must have at least one hex digit
            if self.is_eof() || !self.peek().is_ascii_hexdigit() {
                let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                return Err(LexError::new(LexErrorKind::InvalidHexLiteral, span));
            }

            while !self.is_eof() && self.peek().is_ascii_hexdigit() {
                self.advance();
            }

            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            let text: String = self.source[char_start..self.pos].iter().collect();
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

        let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
        let text: String = self.source[char_start..self.pos].iter().collect();

        let kind = if is_float {
            TokenKind::FloatLit
        } else {
            TokenKind::IntLit
        };

        Ok(Token::new(kind, span, text))
    }

    /// Lex a character literal (`'A'`, `'\n'`, `'\x41'`, `'\u{1F600}'`).
    /// Decodes to the code point and emits an `IntLit` token, so the parser
    /// treats it identically to an integer literal.
    fn lex_char(&mut self) -> LexResult<Token> {
        let start = self.byte_pos;
        self.advance(); // consume opening '

        if self.is_eof() {
            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            return Err(LexError::new(LexErrorKind::UnterminatedChar, span));
        }

        if self.peek() == '\'' {
            self.advance(); // consume closing '
            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            return Err(LexError::new(
                LexErrorKind::InvalidCharLiteral(
                    "empty character literal; use a single character or escape sequence",
                ),
                span,
            ));
        }

        let value: u32 = if self.peek() == '\\' {
            self.advance(); // consume backslash

            if self.is_eof() {
                let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                return Err(LexError::new(LexErrorKind::UnterminatedChar, span));
            }

            let esc = self.peek();
            match esc {
                'n' => {
                    self.advance();
                    '\n' as u32
                }
                't' => {
                    self.advance();
                    '\t' as u32
                }
                'r' => {
                    self.advance();
                    '\r' as u32
                }
                '\\' => {
                    self.advance();
                    '\\' as u32
                }
                '\'' => {
                    self.advance();
                    '\'' as u32
                }
                '"' => {
                    self.advance();
                    '"' as u32
                }
                '$' => {
                    self.advance();
                    '$' as u32
                }
                '0' => {
                    self.advance();
                    0
                }
                'e' => {
                    self.advance();
                    0x1b
                }
                'x' => {
                    self.advance();
                    self.parse_hex_escape(start)? as u32
                }
                'u' => {
                    self.advance();
                    self.parse_unicode_escape(start)? as u32
                }
                _ => {
                    let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                    return Err(LexError::new(LexErrorKind::InvalidEscape(esc), span));
                }
            }
        } else {
            let ch = self.advance();
            if ch == '\n' {
                let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                return Err(LexError::new(LexErrorKind::UnterminatedChar, span));
            }
            if !ch.is_ascii() {
                let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                return Err(LexError::new(
                    LexErrorKind::InvalidCharLiteral(
                        "character literal must contain exactly one ASCII character; use a Unicode escape like \\u{...} for non-ASCII code points",
                    ),
                    span,
                ));
            }
            ch as u32
        };

        if self.is_eof() {
            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            return Err(LexError::new(LexErrorKind::UnterminatedChar, span));
        }

        if self.peek() != '\'' {
            while !self.is_eof() && self.peek() != '\'' && self.peek() != '\n' {
                self.advance();
            }
            if !self.is_eof() && self.peek() == '\'' {
                self.advance();
            }
            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            return Err(LexError::new(
                LexErrorKind::InvalidCharLiteral(
                    "character literal must contain exactly one ASCII character or escape sequence",
                ),
                span,
            ));
        }

        self.advance(); // consume closing '
        let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
        Ok(Token::new(TokenKind::IntLit, span, value.to_string()))
    }

    fn lex_string(&mut self) -> LexResult<Token> {
        let start = self.byte_pos;
        self.advance(); // consume opening "

        let mut value = String::new();
        let mut has_interpolation = false;

        while !self.is_eof() && self.peek() != '"' {
            if self.peek() == '\\' {
                let escaped = self.parse_escape_sequence()?;
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
            self.interpolation_stack.push((1, StringKind::Cooked));

            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            Ok(Token::new(TokenKind::StringStart, span, value))
        } else {
            // Simple string without interpolation
            if self.is_eof() {
                let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                return Err(LexError::new(LexErrorKind::UnterminatedString, span));
            }

            self.advance(); // consume closing "
            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            Ok(Token::new(TokenKind::StringLit, span, value))
        }
    }

    /// Raw single-line string: `\` is literal (no escapes), `"` terminates, `${`
    /// interpolates. Mirrors `lex_string` but never decodes escapes.
    fn lex_raw_string(&mut self) -> LexResult<Token> {
        let start = self.byte_pos;
        self.advance(); // consume r
        self.advance(); // consume opening "

        let (value, terminated, has_interpolation) = self.scan_raw_segment();

        if has_interpolation {
            self.advance(); // consume $
            self.advance(); // consume {
            self.interpolation_stack.push((1, StringKind::Raw));

            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            Ok(Token::new(TokenKind::StringStart, span, value))
        } else {
            if !terminated {
                let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                return Err(LexError::new(LexErrorKind::UnterminatedString, span));
            }

            self.advance(); // consume closing "
            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            Ok(Token::new(TokenKind::StringLit, span, value))
        }
    }

    /// Scan a raw segment up to the closing `"`, an opening `${`, or a line break.
    /// Returns `(value, terminated, found_interp)`. The cursor stops on the
    /// terminator/`$` without consuming it (callers advance as needed).
    fn scan_raw_segment(&mut self) -> (String, bool, bool) {
        let mut value = String::new();

        while !self.is_eof() {
            let ch = self.peek();
            if ch == '"' {
                return (value, true, false);
            }
            if ch == '$' && self.peek_ahead(1) == '{' {
                return (value, false, true);
            }
            if ch == '\n' {
                break;
            }
            value.push(self.advance());
        }

        (value, false, false)
    }

    fn lex_string_continuation(&mut self) -> LexResult<Token> {
        // We're inside a string interpolation, lexing tokens until we hit a closing }
        let (brace_depth, host) = *self
            .interpolation_stack
            .last()
            .expect("interpolation stack must be non-empty");

        // First, check if we're at a brace
        if self.peek() == '{' {
            let top = self
                .interpolation_stack
                .last_mut()
                .expect("interpolation stack must be non-empty");
            top.0 += 1;
            return Ok(self.lex_single(TokenKind::LBrace));
        }

        if self.peek() == '}' {
            // Guard against underflow
            if brace_depth == 0 {
                let start = self.byte_pos;
                self.advance();
                let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                return Err(LexError::new(LexErrorKind::UnexpectedChar('}'), span));
            }

            let new_depth = brace_depth - 1;

            if new_depth == 0 {
                // End of interpolation, resume string
                self.interpolation_stack.pop();
                let start = self.byte_pos;
                self.advance(); // consume }

                let (value, terminated, has_more_interpolation) = match host {
                    StringKind::Cooked => {
                        let mut value = String::new();
                        let mut found_interp = false;
                        while !self.is_eof() && self.peek() != '"' {
                            if self.peek() == '\\' {
                                let escaped = self.parse_escape_sequence()?;
                                value.push(escaped);
                            } else if self.peek() == '$' && self.peek_ahead(1) == '{' {
                                found_interp = true;
                                break;
                            } else {
                                value.push(self.advance());
                            }
                        }
                        (value, !self.is_eof() && !found_interp, found_interp)
                    }
                    StringKind::Raw => self.scan_raw_segment(),
                };

                if has_more_interpolation {
                    self.advance(); // consume $
                    self.advance(); // consume {
                    self.interpolation_stack.push((1, host));

                    let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                    Ok(Token::new(TokenKind::StringContinue, span, value))
                } else {
                    if !terminated {
                        let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                        return Err(LexError::new(LexErrorKind::UnterminatedString, span));
                    }

                    self.advance(); // consume closing "

                    let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                    Ok(Token::new(TokenKind::StringEnd, span, value))
                }
            } else {
                let top = self
                    .interpolation_stack
                    .last_mut()
                    .expect("interpolation stack must be non-empty");
                top.0 = new_depth;
                Ok(self.lex_single(TokenKind::RBrace))
            }
        } else {
            // Regular token inside interpolation.
            self.lex_regular_token()
        }
    }

    fn parse_escape_sequence(&mut self) -> LexResult<char> {
        debug_assert_eq!(
            self.peek(),
            '\\',
            "parse_escape_sequence must be called at a backslash"
        );
        let start = self.byte_pos;
        self.advance(); // consume '\'

        if self.is_eof() {
            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            return Err(LexError::new(LexErrorKind::UnterminatedString, span));
        }

        let ch = self.advance();

        match ch {
            'n' => Ok('\n'),
            't' => Ok('\t'),
            'r' => Ok('\r'),
            '"' => Ok('"'),
            '\\' => Ok('\\'),
            '$' => Ok('$'),
            'e' => Ok('\u{001b}'),
            'x' => self.parse_hex_escape(start),
            'u' => self.parse_unicode_escape(start),
            _ => {
                let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                Err(LexError::new(LexErrorKind::InvalidEscape(ch), span))
            }
        }
    }

    fn parse_hex_escape(&mut self, start: usize) -> LexResult<char> {
        let first = self.consume_required_hex_digit(start)?;
        let second = self.consume_required_hex_digit(start)?;
        let value = (first << 4) | second;

        if value > 0x7f {
            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            return Err(LexError::new(LexErrorKind::InvalidHexEscape, span));
        }

        Ok(char::from_u32(value).expect("ASCII code point must be valid"))
    }

    fn consume_required_hex_digit(&mut self, start: usize) -> LexResult<u32> {
        if self.is_eof() {
            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            return Err(LexError::new(LexErrorKind::InvalidHexEscape, span));
        }

        let ch = self.peek();
        if !ch.is_ascii_hexdigit() {
            // Include obvious malformed content in the escape span,
            // but avoid consuming a string terminator.
            if ch != '"' {
                self.advance();
            }
            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            return Err(LexError::new(LexErrorKind::InvalidHexEscape, span));
        }

        self.advance();
        Ok(ch.to_digit(16).expect("hex digit must convert"))
    }

    fn parse_unicode_escape(&mut self, start: usize) -> LexResult<char> {
        if self.is_eof() || self.peek() != '{' {
            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            return Err(LexError::new(LexErrorKind::InvalidUnicodeEscape, span));
        }

        self.advance(); // consume '{'
        let mut value: u32 = 0;
        let mut digits = 0usize;

        loop {
            if self.is_eof() {
                let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                return Err(LexError::new(LexErrorKind::InvalidUnicodeEscape, span));
            }

            let ch = self.peek();
            if ch == '}' {
                self.advance(); // consume '}'
                break;
            }

            if !ch.is_ascii_hexdigit() {
                // Include malformed content in the span, but do not consume a
                // string terminator.
                if ch != '"' {
                    self.advance();
                }
                let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                return Err(LexError::new(LexErrorKind::InvalidUnicodeEscape, span));
            }

            if digits == 6 {
                self.advance(); // consume the overflow digit for better span coverage
                let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                return Err(LexError::new(LexErrorKind::InvalidUnicodeEscape, span));
            }

            self.advance();
            digits += 1;
            value = (value << 4) | ch.to_digit(16).expect("hex digit must convert");
        }

        if digits == 0 {
            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            return Err(LexError::new(LexErrorKind::InvalidUnicodeEscape, span));
        }

        if (0xd800..=0xdfff).contains(&value) {
            let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
            return Err(LexError::new(LexErrorKind::InvalidUnicodeEscape, span));
        }

        match char::from_u32(value) {
            Some(ch) => Ok(ch),
            None => {
                let span = Span::new(self.file_id, start as u32, self.byte_pos as u32);
                Err(LexError::new(LexErrorKind::InvalidUnicodeEscape, span))
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
        self.byte_pos += ch.len_utf8();
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
    fn test_lex_char_literal() {
        // A character literal lexes to an IntLit token holding its code point.
        let tokens = lex_simple("'A' '0' ' '");
        assert_eq!(tokens[0].kind, TokenKind::IntLit);
        assert_eq!(tokens[0].text, "65");
        assert_eq!(tokens[1].kind, TokenKind::IntLit);
        assert_eq!(tokens[1].text, "48");
        assert_eq!(tokens[2].kind, TokenKind::IntLit);
        assert_eq!(tokens[2].text, "32");
    }

    #[test]
    fn test_lex_char_literal_escapes() {
        let tokens = lex_simple(r"'\n' '\t' '\\' '\'' '\$' '\0' '\e' '\x41' '\u{1F600}'");
        let codes: Vec<&str> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::IntLit)
            .map(|t| t.text.as_str())
            .collect();
        assert_eq!(
            codes,
            vec!["10", "9", "92", "39", "36", "0", "27", "65", "128512"]
        );
    }

    #[test]
    fn test_lex_char_literal_unterminated() {
        let result = Lexer::lex("'A", FileId(0));
        assert_eq!(result.unwrap_err().kind, LexErrorKind::UnterminatedChar);
    }

    #[test]
    fn test_lex_char_literal_empty() {
        let result = Lexer::lex("''", FileId(0));
        assert_eq!(
            result.unwrap_err().kind,
            LexErrorKind::InvalidCharLiteral(
                "empty character literal; use a single character or escape sequence"
            )
        );
    }

    #[test]
    fn test_lex_char_literal_invalid_content() {
        let result = Lexer::lex("'ab'", FileId(0));
        assert_eq!(
            result.unwrap_err().kind,
            LexErrorKind::InvalidCharLiteral(
                "character literal must contain exactly one ASCII character or escape sequence"
            )
        );

        let result = Lexer::lex("'é'", FileId(0));
        assert_eq!(
            result.unwrap_err().kind,
            LexErrorKind::InvalidCharLiteral(
                "character literal must contain exactly one ASCII character; use a Unicode escape like \\u{...} for non-ASCII code points"
            )
        );
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
    fn test_lex_string_hex_and_escape_alias() {
        let tokens = lex_simple(r#""\x1b-\x0A-\e""#);
        assert_eq!(tokens[0].kind, TokenKind::StringLit);
        assert_eq!(tokens[0].text, "\u{001b}-\n-\u{001b}");
    }

    #[test]
    fn test_lex_string_invalid_hex_escape_no_digits() {
        let result = Lexer::lex(r#""\x""#, FileId(0));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, LexErrorKind::InvalidHexEscape);
    }

    #[test]
    fn test_lex_string_invalid_hex_escape_non_hex_digit() {
        let result = Lexer::lex(r#""\xG0""#, FileId(0));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, LexErrorKind::InvalidHexEscape);
    }

    #[test]
    fn test_lex_string_invalid_hex_escape_out_of_ascii() {
        let result = Lexer::lex(r#""\x80""#, FileId(0));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, LexErrorKind::InvalidHexEscape);
    }

    #[test]
    fn test_lex_string_invalid_hex_escape_unterminated() {
        let result = Lexer::lex(r#""\x1"#, FileId(0));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, LexErrorKind::InvalidHexEscape);
    }

    #[test]
    fn test_lex_raw_string_keeps_backslashes() {
        let tokens = lex_simple(r#"r"\d+""#);
        assert_eq!(tokens[0].kind, TokenKind::StringLit);
        assert_eq!(tokens[0].text, r"\d+");
    }

    #[test]
    fn test_lex_raw_string_interpolation_stays_raw() {
        let tokens = lex_simple(r#"r"\d ${x} z\w""#);
        assert_eq!(tokens[0].kind, TokenKind::StringStart);
        assert_eq!(tokens[0].text, r"\d ");
        assert_eq!(tokens[1].kind, TokenKind::Ident);
        assert_eq!(tokens[1].text, "x");
        assert_eq!(tokens[2].kind, TokenKind::StringEnd);
        assert_eq!(tokens[2].text, r" z\w");
    }

    #[test]
    fn test_raw_prefix_only_before_quote() {
        let tokens = lex_simple("red");
        assert_eq!(tokens[0].kind, TokenKind::Ident);
        assert_eq!(tokens[0].text, "red");
    }

    #[test]
    fn test_lex_string_interpolation_keeps_dollar_escape() {
        let tokens = lex_simple(r#""cost=\$${n}""#);
        assert_eq!(tokens[0].kind, TokenKind::StringStart);
        assert_eq!(tokens[0].text, "cost=$");
        assert_eq!(tokens[1].kind, TokenKind::Ident);
        assert_eq!(tokens[1].text, "n");
        assert_eq!(tokens[2].kind, TokenKind::StringEnd);
        assert_eq!(tokens[2].text, "");
    }

    #[test]
    fn test_lex_string_interpolation_continuation_uses_hex_escape() {
        let tokens = lex_simple(r#""${name}\x1b""#);
        assert_eq!(tokens[0].kind, TokenKind::StringStart);
        assert_eq!(tokens[0].text, "");
        assert_eq!(tokens[1].kind, TokenKind::Ident);
        assert_eq!(tokens[1].text, "name");
        assert_eq!(tokens[2].kind, TokenKind::StringEnd);
        assert_eq!(tokens[2].text, "\u{001b}");
    }

    #[test]
    fn test_lex_string_unicode_escape_valid() {
        let tokens = lex_simple(r#""\u{41}\u{00E9}\u{1F44D}""#);
        assert_eq!(tokens[0].kind, TokenKind::StringLit);
        assert_eq!(tokens[0].text, "Aé👍");
    }

    #[test]
    fn test_lex_string_unicode_escape_invalid_missing_brace() {
        let result = Lexer::lex(r#""\u41""#, FileId(0));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, LexErrorKind::InvalidUnicodeEscape);
    }

    #[test]
    fn test_lex_string_unicode_escape_invalid_empty_digits() {
        let result = Lexer::lex(r#""\u{}""#, FileId(0));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, LexErrorKind::InvalidUnicodeEscape);
    }

    #[test]
    fn test_lex_string_unicode_escape_invalid_non_hex_digit() {
        let result = Lexer::lex(r#""\u{ZZ}""#, FileId(0));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, LexErrorKind::InvalidUnicodeEscape);
    }

    #[test]
    fn test_lex_string_unicode_escape_invalid_too_many_digits() {
        let result = Lexer::lex(r#""\u{1234567}""#, FileId(0));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, LexErrorKind::InvalidUnicodeEscape);
    }

    #[test]
    fn test_lex_string_unicode_escape_invalid_surrogate() {
        let result = Lexer::lex(r#""\u{D800}""#, FileId(0));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, LexErrorKind::InvalidUnicodeEscape);
    }

    #[test]
    fn test_lex_string_unicode_escape_invalid_out_of_range() {
        let result = Lexer::lex(r#""\u{110000}""#, FileId(0));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, LexErrorKind::InvalidUnicodeEscape);
    }

    #[test]
    fn test_lex_string_unicode_escape_invalid_unterminated() {
        let result = Lexer::lex(r#""\u{1F44D""#, FileId(0));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, LexErrorKind::InvalidUnicodeEscape);
    }

    #[test]
    fn test_lex_string_interpolation_continuation_uses_unicode_escape() {
        let tokens = lex_simple(r#""${name}\u{1F44D}""#);
        assert_eq!(tokens[0].kind, TokenKind::StringStart);
        assert_eq!(tokens[0].text, "");
        assert_eq!(tokens[1].kind, TokenKind::Ident);
        assert_eq!(tokens[1].text, "name");
        assert_eq!(tokens[2].kind, TokenKind::StringEnd);
        assert_eq!(tokens[2].text, "👍");
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
    fn test_lex_nested_interpolation_keeps_outer_context() {
        let source = r#"a := "${dim("x ${msg}")}"
b := "${passed} passed""#;
        let tokens = lex_simple(source);

        let b_pos = tokens
            .iter()
            .position(|t| t.kind == TokenKind::Ident && t.text == "b")
            .expect("expected second binding");

        assert!(
            tokens[b_pos..]
                .iter()
                .any(|t| t.kind == TokenKind::StringStart),
            "expected interpolation in second string to lex successfully"
        );
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

    #[test]
    fn test_lex_dotdot() {
        // `..` emits a single DotDot token
        let tokens = lex_simple("..");
        assert_eq!(tokens[0].kind, TokenKind::DotDot);
        assert_eq!(tokens[0].text, "..");

        // `1..10` → IntLit DotDot IntLit
        let tokens = lex_simple("1..10");
        assert_eq!(tokens[0].kind, TokenKind::IntLit);
        assert_eq!(tokens[1].kind, TokenKind::DotDot);
        assert_eq!(tokens[2].kind, TokenKind::IntLit);

        // `a..b` → Ident DotDot Ident (no float confusion)
        let tokens = lex_simple("a..b");
        assert_eq!(tokens[0].kind, TokenKind::Ident);
        assert_eq!(tokens[1].kind, TokenKind::DotDot);
        assert_eq!(tokens[2].kind, TokenKind::Ident);

        // `1.5..10` → FloatLit DotDot IntLit (float then range, not confused)
        let tokens = lex_simple("1.5..10");
        assert_eq!(tokens[0].kind, TokenKind::FloatLit);
        assert_eq!(tokens[0].text, "1.5");
        assert_eq!(tokens[1].kind, TokenKind::DotDot);
        assert_eq!(tokens[2].kind, TokenKind::IntLit);

        // `.` alone still emits Dot
        let tokens = lex_simple("a.b");
        assert_eq!(tokens[0].kind, TokenKind::Ident);
        assert_eq!(tokens[1].kind, TokenKind::Dot);
        assert_eq!(tokens[2].kind, TokenKind::Ident);
    }
}
