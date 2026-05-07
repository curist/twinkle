use crate::syntax::span::Span;
use std::fmt;

/// All token types in Twinkle
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    // Keywords (17 total)
    Fn,
    If,
    Else,
    Type,
    Case,
    For,
    In,
    Collect,
    Break,
    Continue,
    Return,
    Use,
    As,
    Pub,
    Try,
    And,
    Or,
    True,
    False,
    Defer,
    Extern,

    // Literals
    IntLit,
    FloatLit,
    StringLit,      // Simple string without interpolation
    StringStart,    // String with interpolation: "text ${
    StringContinue, // Middle part: } text ${
    StringEnd,      // End part: } text"

    // Identifiers
    Ident,

    // Operators - Arithmetic
    Plus,    // +
    Minus,   // -
    Star,    // *
    Slash,   // /
    Percent, // %

    // Operators - Comparison
    EqEq,   // ==
    BangEq, // !=
    Lt,     // <
    LtEq,   // <=
    Gt,     // >
    GtEq,   // >=

    // Operators - Bitwise
    Amp,   // &
    Pipe,  // |
    Caret, // ^
    Tilde, // ~

    // Operators - Logical
    Bang, // !

    // Assignment
    Eq,      // =
    ColonEq, // :=

    // Punctuation
    LParen,     // (
    RParen,     // )
    LBrace,     // {
    RBrace,     // }
    LBracket,   // [
    RBracket,   // ]
    Comma,      // ,
    Colon,      // :
    Dot,        // .
    DotDot,     // ..
    Question,   // ?
    Semi,       // ;
    FatArrow,   // =>
    Underscore, // _

    // Special
    DollarBrace, // ${ for string interpolation
    At,          // @ for stdlib prefix

    // Meta
    Eof,
    Error,
}

impl TokenKind {
    /// Check if this token is a keyword
    pub fn is_keyword(&self) -> bool {
        matches!(
            self,
            TokenKind::Fn
                | TokenKind::If
                | TokenKind::Else
                | TokenKind::Type
                | TokenKind::Case
                | TokenKind::For
                | TokenKind::In
                | TokenKind::Collect
                | TokenKind::Break
                | TokenKind::Continue
                | TokenKind::Return
                | TokenKind::Use
                | TokenKind::As
                | TokenKind::Pub
                | TokenKind::Try
                | TokenKind::And
                | TokenKind::Or
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Defer
                | TokenKind::Extern
        )
    }

    /// Check if this token is a literal
    pub fn is_literal(&self) -> bool {
        matches!(
            self,
            TokenKind::IntLit
                | TokenKind::FloatLit
                | TokenKind::StringLit
                | TokenKind::StringStart
                | TokenKind::True
                | TokenKind::False
        )
    }

    /// Get keyword from identifier text
    pub fn from_keyword(text: &str) -> Option<TokenKind> {
        Some(match text {
            "fn" => TokenKind::Fn,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "type" => TokenKind::Type,
            "case" => TokenKind::Case,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "collect" => TokenKind::Collect,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "return" => TokenKind::Return,
            "use" => TokenKind::Use,
            "as" => TokenKind::As,
            "pub" => TokenKind::Pub,
            "try" => TokenKind::Try,
            "and" => TokenKind::And,
            "or" => TokenKind::Or,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "defer" => TokenKind::Defer,
            "extern" => TokenKind::Extern,
            _ => return None,
        })
    }

    /// Get a human-readable name for the token
    pub fn name(&self) -> &'static str {
        match self {
            TokenKind::Fn => "fn",
            TokenKind::If => "if",
            TokenKind::Else => "else",
            TokenKind::Type => "type",
            TokenKind::Case => "case",
            TokenKind::For => "for",
            TokenKind::In => "in",
            TokenKind::Collect => "collect",
            TokenKind::Break => "break",
            TokenKind::Continue => "continue",
            TokenKind::Return => "return",
            TokenKind::Use => "use",
            TokenKind::As => "as",
            TokenKind::Pub => "pub",
            TokenKind::Try => "try",
            TokenKind::And => "and",
            TokenKind::Or => "or",
            TokenKind::True => "true",
            TokenKind::False => "false",
            TokenKind::Defer => "defer",
            TokenKind::Extern => "extern",
            TokenKind::IntLit => "integer literal",
            TokenKind::FloatLit => "float literal",
            TokenKind::StringLit => "string literal",
            TokenKind::StringStart => "string start",
            TokenKind::StringContinue => "string continue",
            TokenKind::StringEnd => "string end",
            TokenKind::Ident => "identifier",
            TokenKind::Plus => "+",
            TokenKind::Minus => "-",
            TokenKind::Star => "*",
            TokenKind::Slash => "/",
            TokenKind::Percent => "%",
            TokenKind::EqEq => "==",
            TokenKind::BangEq => "!=",
            TokenKind::Lt => "<",
            TokenKind::LtEq => "<=",
            TokenKind::Gt => ">",
            TokenKind::GtEq => ">=",
            TokenKind::Amp => "&",
            TokenKind::Pipe => "|",
            TokenKind::Caret => "^",
            TokenKind::Tilde => "~",
            TokenKind::Bang => "!",
            TokenKind::Eq => "=",
            TokenKind::ColonEq => ":=",
            TokenKind::LParen => "(",
            TokenKind::RParen => ")",
            TokenKind::LBrace => "{",
            TokenKind::RBrace => "}",
            TokenKind::LBracket => "[",
            TokenKind::RBracket => "]",
            TokenKind::Comma => ",",
            TokenKind::Colon => ":",
            TokenKind::Dot => ".",
            TokenKind::DotDot => "..",
            TokenKind::Question => "?",
            TokenKind::Semi => ";",
            TokenKind::FatArrow => "=>",
            TokenKind::Underscore => "_",
            TokenKind::DollarBrace => "${",
            TokenKind::At => "@",
            TokenKind::Eof => "end of file",
            TokenKind::Error => "error",
        }
    }
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// A token with its kind, span, and source text
#[derive(Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub text: String,
    /// True if at least one newline appeared before this token (after the previous token).
    pub preceded_by_newline: bool,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span, text: String) -> Self {
        Self {
            kind,
            span,
            text,
            preceded_by_newline: false,
        }
    }

    /// Create an EOF token
    pub fn eof(file_id: crate::syntax::span::FileId, offset: u32) -> Self {
        Self {
            kind: TokenKind::Eof,
            span: Span::new(file_id, offset, offset),
            text: String::new(),
            preceded_by_newline: false,
        }
    }

    /// Create an error token
    pub fn error(span: Span, text: String) -> Self {
        Self {
            kind: TokenKind::Error,
            span,
            text,
            preceded_by_newline: false,
        }
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.text.is_empty() {
            write!(f, "{:?} @ {:?}", self.kind, self.span)
        } else {
            write!(f, "{:?}({:?}) @ {:?}", self.kind, self.text, self.span)
        }
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.text.is_empty() {
            write!(f, "{}", self.kind)
        } else {
            write!(f, "{} '{}'", self.kind, self.text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::span::FileId;

    #[test]
    fn test_keyword_recognition() {
        assert_eq!(TokenKind::from_keyword("fn"), Some(TokenKind::Fn));
        assert_eq!(TokenKind::from_keyword("if"), Some(TokenKind::If));
        assert_eq!(TokenKind::from_keyword("use"), Some(TokenKind::Use));
        assert_eq!(TokenKind::from_keyword("as"), Some(TokenKind::As));
        assert_eq!(TokenKind::from_keyword("true"), Some(TokenKind::True));
        assert_eq!(TokenKind::from_keyword("false"), Some(TokenKind::False));
        assert_eq!(TokenKind::from_keyword("not_a_keyword"), None);
    }

    #[test]
    fn test_token_creation() {
        let file_id = FileId(0);
        let span = Span::new(file_id, 0, 2);
        let token = Token::new(TokenKind::Fn, span, "fn".to_string());

        assert_eq!(token.kind, TokenKind::Fn);
        assert_eq!(token.text, "fn");
        assert_eq!(token.span, span);
    }

    #[test]
    fn test_eof_token() {
        let file_id = FileId(0);
        let eof = Token::eof(file_id, 100);

        assert_eq!(eof.kind, TokenKind::Eof);
        assert_eq!(eof.span.start, 100);
        assert_eq!(eof.span.end, 100);
    }
}
