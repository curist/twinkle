use super::ast::ExprId;
use crate::syntax::ast::*;
use crate::syntax::span::{FileId, Span};
use crate::syntax::tokens::{Token, TokenKind};

pub type ParseResult<T> = Result<T, ParseError>;

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParseErrorKind {
    UnexpectedToken {
        expected: Vec<String>,
        found: String,
    },
    UnexpectedEof {
        expected: Vec<String>,
    },
    /// `.Upper` appeared in postfix position (after an expression).
    /// Variant/constructor names must appear in prefix form only.
    ConstructorInPostfix {
        name: String,
    },
    /// A variant literal used a lowercase name (must be PascalCase).
    LowercaseVariant {
        name: String,
    },
    /// A declaration name violates initial-case rules.
    CaseViolation {
        kind: &'static str,
        name: String,
        expected: &'static str,
    },
    /// Empty destructuring import list: `use foo.{}`
    EmptyImportList,
    /// A statement keyword appeared where an expression is required.
    StatementInExpression {
        statement: &'static str,
        context: Option<&'static str>,
    },
}

impl ParseError {
    fn new(kind: ParseErrorKind, span: Span) -> Self {
        Self { kind, span }
    }

    fn with_context(mut self, ctx: &'static str) -> Self {
        if let ParseErrorKind::StatementInExpression {
            ref mut context, ..
        } = self.kind
        {
            *context = Some(ctx);
        }
        self
    }

    fn unexpected_token(expected: Vec<&str>, found: &Token) -> Self {
        Self::new(
            ParseErrorKind::UnexpectedToken {
                expected: expected.iter().map(|s| s.to_string()).collect(),
                found: found.kind.name().to_string(),
            },
            found.span,
        )
    }

    fn unexpected_eof(span: Span, expected: Vec<&str>) -> Self {
        Self::new(
            ParseErrorKind::UnexpectedEof {
                expected: expected.iter().map(|s| s.to_string()).collect(),
            },
            span,
        )
    }
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    file_id: FileId,
    next_expr_id: u32,
}

impl Parser {
    pub fn new(tokens: Vec<Token>, file_id: FileId) -> Self {
        Self {
            tokens,
            pos: 0,
            file_id,
            next_expr_id: 0,
        }
    }

    /// Allocate a new unique ExprId
    fn alloc_expr_id(&mut self) -> ExprId {
        let id = ExprId(self.next_expr_id);
        self.next_expr_id += 1;
        id
    }

    // Core token navigation

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn peek_kind(&self) -> Option<TokenKind> {
        self.peek().map(|t| t.kind)
    }

    /// Returns true when the current token and the next token form an adjacent
    /// pair of the same kind (`<`+`<` or `>`+`>`), meaning no whitespace
    /// separates them (first token's end == second token's start).
    fn is_adjacent_shift(&self, expected: TokenKind) -> bool {
        if let (Some(first), Some(second)) =
            (self.tokens.get(self.pos), self.tokens.get(self.pos + 1))
        {
            second.kind == expected && first.span.end == second.span.start
        } else {
            false
        }
    }

    fn advance(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn is_eof(&self) -> bool {
        matches!(self.peek_kind(), Some(TokenKind::Eof) | None)
    }

    fn expect(&mut self, kind: TokenKind) -> ParseResult<Token> {
        match self.peek() {
            Some(token) if token.kind == kind => Ok(self.advance().unwrap()),
            Some(token) => Err(ParseError::unexpected_token(vec![kind.name()], token)),
            None => {
                let span = self.eof_span();
                Err(ParseError::unexpected_eof(span, vec![kind.name()]))
            }
        }
    }

    fn peek_is(&self, kind: TokenKind) -> bool {
        self.peek_kind() == Some(kind)
    }

    fn consume_if(&mut self, kind: TokenKind) -> bool {
        if self.peek_is(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn eof_span(&self) -> Span {
        self.tokens
            .last()
            .map(|t| t.span)
            .unwrap_or_else(|| Span::new(self.file_id, 0, 0))
    }

    // Top-level parsing

    pub fn parse_source_file(&mut self) -> ParseResult<SourceFile> {
        let start_span = self
            .peek()
            .map(|t| t.span)
            .unwrap_or_else(|| self.eof_span());
        let mut items = Vec::new();

        while !self.is_eof() {
            items.push(self.parse_item()?);
        }

        let end_span = items
            .last()
            .map(|item| match item {
                Item::Import(i) => i.span,
                Item::TypeDecl(t) => t.span,
                Item::Function(f) => f.span,
                Item::Stmt(s) => match s {
                    Stmt::Let { span, .. } => *span,
                    Stmt::For { span, .. } => *span,
                    Stmt::ForCond { span, .. } => *span,
                    Stmt::Expr(e) => e.span,
                    Stmt::Break { span, .. } => *span,
                    Stmt::Continue { span } => *span,
                    Stmt::Return { span, .. } => *span,
                    Stmt::Defer { span, .. } => *span,
                },
            })
            .unwrap_or(start_span);

        let span = start_span.merge(&end_span);

        Ok(SourceFile { items, span })
    }

    fn parse_item(&mut self) -> ParseResult<Item> {
        // Check for pub modifier
        let is_pub = if self.peek_is(TokenKind::Pub) {
            self.advance();
            true
        } else {
            false
        };

        match self.peek_kind() {
            Some(TokenKind::Use) => {
                if is_pub {
                    // pub use not supported — silently ignore pub modifier
                }
                Ok(Item::Import(self.parse_use_decl()?))
            }
            Some(TokenKind::Type) => Ok(Item::TypeDecl(self.parse_type_decl(is_pub)?)),
            Some(TokenKind::Fn) => Ok(Item::Function(self.parse_function_decl(is_pub)?)),
            _ => {
                if is_pub {
                    // Only let bindings can be `pub` at module scope
                    if self.is_let_binding() {
                        return Ok(Item::Stmt(self.parse_let_stmt(true)?));
                    }
                    return Err(ParseError::unexpected_token(
                        vec!["fn", "type", "let binding"],
                        self.peek().unwrap(),
                    ));
                }
                // Top-level statement
                Ok(Item::Stmt(self.parse_stmt()?))
            }
        }
    }

    fn parse_use_decl(&mut self) -> ParseResult<ImportDecl> {
        let start = self.expect(TokenKind::Use)?;

        // Optional @ prefix for stdlib
        let is_stdlib = if self.peek_is(TokenKind::At) {
            self.advance();
            true
        } else {
            false
        };

        // Optional leading dot for relative import: `use .foo`
        let is_relative = if !is_stdlib && self.peek_is(TokenKind::Dot) {
            self.advance();
            true
        } else {
            false
        };

        // Parse dot-separated path: foo.bar.baz
        let first = self.expect(TokenKind::Ident)?;
        let mut module_path = vec![first.text.clone()];
        let mut last_span = first.span;
        let mut module_name_span = first.span; // span of the last path segment (for diagnostics)

        while self.peek_is(TokenKind::Dot) {
            // Peek ahead to see if after the dot there's an Ident (not { or .)
            let next_after_dot = self.tokens.get(self.pos + 1);
            match next_after_dot.map(|t| t.kind) {
                Some(TokenKind::Ident) => {
                    self.advance(); // consume dot
                    let seg = self.expect(TokenKind::Ident)?;
                    module_path.push(seg.text.clone());
                    last_span = seg.span;
                    module_name_span = seg.span;
                }
                _ => break,
            }
        }

        // Check for destructuring import: `.{...}`
        let items = if self.peek_is(TokenKind::Dot) {
            let next_after_dot = self.tokens.get(self.pos + 1);
            if matches!(next_after_dot.map(|t| t.kind), Some(TokenKind::LBrace)) {
                self.advance(); // consume dot
                self.advance(); // consume {
                let items = self.parse_import_items()?;
                let rbrace = self.expect(TokenKind::RBrace)?;
                last_span = rbrace.span;
                Some(items)
            } else {
                None
            }
        } else {
            None
        };

        // Optional alias: as alias_name (only if no item list)
        let alias = if items.is_none() && self.peek_is(TokenKind::As) {
            self.advance(); // consume 'as'
            let alias_tok = self.expect(TokenKind::Ident)?;
            if alias_tok.text.starts_with(|c: char| c.is_uppercase()) {
                return Err(ParseError::new(
                    ParseErrorKind::CaseViolation {
                        kind: "module alias",
                        name: alias_tok.text.clone(),
                        expected: "a lowercase",
                    },
                    alias_tok.span,
                ));
            }
            last_span = alias_tok.span;

            // Reject combined alias + destructuring: `use foo as bar.{...}`
            if self.peek_is(TokenKind::Dot) {
                let next_after_dot = self.tokens.get(self.pos + 1);
                if matches!(next_after_dot.map(|t| t.kind), Some(TokenKind::LBrace)) {
                    return Err(ParseError::unexpected_token(
                        vec!["end of import"],
                        &self.tokens[self.pos],
                    ));
                }
            }

            Some(alias_tok.text.clone())
        } else {
            None
        };

        // Enforce lowercase for the effective module identifier
        // (either the explicit alias or the last path segment)
        if alias.is_none() {
            let module_name = module_path.last().unwrap();
            if module_name.starts_with(|c: char| c.is_uppercase()) {
                return Err(ParseError::new(
                    ParseErrorKind::CaseViolation {
                        kind: "module name",
                        name: module_name.clone(),
                        expected: "a lowercase",
                    },
                    module_name_span,
                ));
            }
        }

        let span = start.span.merge(&last_span);

        Ok(ImportDecl {
            module_path,
            is_stdlib,
            is_relative,
            alias,
            items,
            span,
        })
    }

    /// Parse comma-separated import items inside `{...}`. Expects `{` already consumed.
    fn parse_import_items(&mut self) -> ParseResult<Vec<ImportItem>> {
        let mut items = Vec::new();

        // Reject empty list
        if self.peek_is(TokenKind::RBrace) {
            return Err(ParseError::new(
                ParseErrorKind::EmptyImportList,
                self.tokens[self.pos].span,
            ));
        }

        loop {
            // Trailing comma: `{x,}` — stop before `}`
            if self.peek_is(TokenKind::RBrace) {
                break;
            }

            // Infer value vs type from casing: PascalCase → type, snake_case → value
            let name_tok = self.expect(TokenKind::Ident)?;
            let alias = if self.peek_is(TokenKind::As) {
                self.advance();
                Some(self.expect(TokenKind::Ident)?.text.clone())
            } else {
                None
            };
            let span = name_tok.span.merge(&self.tokens[self.pos - 1].span);
            if name_tok.text.starts_with(|c: char| c.is_uppercase()) {
                items.push(ImportItem::Type {
                    name: name_tok.text.clone(),
                    alias,
                    span,
                });
            } else {
                items.push(ImportItem::Value {
                    name: name_tok.text.clone(),
                    alias,
                    span,
                });
            }

            // Expect comma or end
            if self.peek_is(TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        Ok(items)
    }

    fn parse_type_decl(&mut self, is_pub: bool) -> ParseResult<TypeDecl> {
        let start = self.expect(TokenKind::Type)?;
        let name_token = self.expect(TokenKind::Ident)?;
        let name = name_token.text.clone();

        if name.starts_with(|c: char| c.is_lowercase()) {
            return Err(ParseError::new(
                ParseErrorKind::CaseViolation {
                    kind: "type name",
                    name,
                    expected: "an uppercase",
                },
                name_token.span,
            ));
        }

        // Optional type parameters: <T, U>
        let type_params = if self.peek_is(TokenKind::Lt) {
            self.expect(TokenKind::Lt)?;
            let mut params = Vec::new();

            while !self.peek_is(TokenKind::Gt) && !self.is_eof() {
                let param = self.expect(TokenKind::Ident)?;
                params.push(param.text.clone());

                if !self.peek_is(TokenKind::Gt) {
                    self.expect(TokenKind::Comma)?;
                }
            }

            self.expect(TokenKind::Gt)?;
            params
        } else {
            Vec::new()
        };

        self.expect(TokenKind::Eq)?;

        // Parse type definition: record, sum, or alias
        let definition = if self.peek_is(TokenKind::Dot) {
            // Record type: .{ fields }
            self.expect(TokenKind::Dot)?;
            self.expect(TokenKind::LBrace)?;

            let mut fields = Vec::new();
            while !self.peek_is(TokenKind::RBrace) && !self.is_eof() {
                let field_start = self.expect(TokenKind::Ident)?;
                let field_name = field_start.text.clone();

                if field_name.starts_with(|c: char| c.is_uppercase()) {
                    return Err(ParseError::new(
                        ParseErrorKind::CaseViolation {
                            kind: "record field name",
                            name: field_name,
                            expected: "a lowercase",
                        },
                        field_start.span,
                    ));
                }

                self.expect(TokenKind::Colon)?;
                let field_ty = self.parse_type()?;
                let field_span = field_start.span.merge(&field_ty.span());

                fields.push(RecordField {
                    name: field_name,
                    ty: field_ty,
                    span: field_span,
                });

                if !self.peek_is(TokenKind::RBrace) {
                    self.expect(TokenKind::Comma)?;
                }
            }

            self.expect(TokenKind::RBrace)?;
            TypeDef::Record { fields }
        } else if self.peek_is(TokenKind::LBrace) {
            // Sum type: { variants }
            self.expect(TokenKind::LBrace)?;

            let mut variants = Vec::new();
            while !self.peek_is(TokenKind::RBrace) && !self.is_eof() {
                let variant_start = self.expect(TokenKind::Ident)?;
                let variant_name = variant_start.text.clone();

                // Optional variant fields: Name(Type1, Type2)
                let fields = if self.peek_is(TokenKind::LParen) {
                    self.expect(TokenKind::LParen)?;
                    let mut fields = Vec::new();

                    while !self.peek_is(TokenKind::RParen) && !self.is_eof() {
                        fields.push(self.parse_type()?);

                        if !self.peek_is(TokenKind::RParen) {
                            self.expect(TokenKind::Comma)?;
                        }
                    }

                    self.expect(TokenKind::RParen)?;
                    fields
                } else {
                    Vec::new()
                };

                let variant_span = if let Some(last) = fields.last() {
                    variant_start.span.merge(&last.span())
                } else {
                    variant_start.span
                };

                variants.push(Variant {
                    name: variant_name,
                    fields,
                    span: variant_span,
                });

                if !self.peek_is(TokenKind::RBrace) {
                    self.expect(TokenKind::Comma)?;
                }
            }

            self.expect(TokenKind::RBrace)?;
            TypeDef::Sum { variants }
        } else {
            // Type alias: type Name = OtherType
            let ty = self.parse_type()?;
            TypeDef::Alias { ty }
        };

        let end_span = match &definition {
            TypeDef::Record { fields } => fields.last().map(|f| f.span).unwrap_or(name_token.span),
            TypeDef::Sum { variants } => variants.last().map(|v| v.span).unwrap_or(name_token.span),
            TypeDef::Alias { ty } => ty.span(),
        };

        let span = start.span.merge(&end_span);

        Ok(TypeDecl {
            is_pub,
            doc: None,
            name,
            type_params,
            definition,
            span,
        })
    }

    fn parse_function_decl(&mut self, is_pub: bool) -> ParseResult<FunctionDecl> {
        let start = self.expect(TokenKind::Fn)?;
        let name_token = self.expect(TokenKind::Ident)?;
        let name = name_token.text.clone();

        if name.starts_with(|c: char| c.is_uppercase()) {
            return Err(ParseError::new(
                ParseErrorKind::CaseViolation {
                    kind: "function name",
                    name,
                    expected: "a lowercase",
                },
                name_token.span,
            ));
        }

        // Optional type parameters: <T, U>
        let type_params = if self.peek_is(TokenKind::Lt) {
            self.expect(TokenKind::Lt)?;
            let mut params = Vec::new();

            while !self.peek_is(TokenKind::Gt) && !self.is_eof() {
                let param = self.expect(TokenKind::Ident)?;
                params.push(param.text.clone());

                if !self.peek_is(TokenKind::Gt) {
                    self.expect(TokenKind::Comma)?;
                }
            }

            self.expect(TokenKind::Gt)?;
            params
        } else {
            Vec::new()
        };

        // Parameters
        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();

        while !self.peek_is(TokenKind::RParen) && !self.is_eof() {
            params.push(self.parse_param()?);

            if !self.peek_is(TokenKind::RParen) {
                self.expect(TokenKind::Comma)?;
            }
        }

        self.expect(TokenKind::RParen)?;

        // Optional return type
        let return_type = if !self.peek_is(TokenKind::LBrace) {
            Some(self.parse_type()?)
        } else {
            None
        };

        // Body
        let body = self.parse_block()?;
        let span = start.span.merge(&body.span);

        Ok(FunctionDecl {
            is_pub,
            doc: None,
            name,
            type_params,
            params,
            return_type,
            body,
            span,
        })
    }

    // Expression parsing with Pratt

    pub fn parse_expr(&mut self) -> ParseResult<Expr> {
        self.parse_expr_bp(0)
    }

    fn parse_expr_in(&mut self, context: &'static str) -> ParseResult<Expr> {
        self.parse_expr_bp(0).map_err(|e| e.with_context(context))
    }

    /// Ensure all tokens have been consumed
    pub fn expect_eof(&mut self) -> ParseResult<()> {
        if !self.is_eof() {
            let token = self.peek().unwrap();
            return Err(ParseError::unexpected_token(vec!["end of file"], token));
        }
        Ok(())
    }

    fn parse_expr_bp(&mut self, min_bp: u8) -> ParseResult<Expr> {
        // Parse prefix
        let mut lhs = self.parse_prefix()?;

        // Parse infix and postfix operators
        loop {
            let op_kind = match self.peek_kind() {
                Some(kind) => kind,
                None => break,
            };

            // Check for shift operators: adjacent `<`+`<` or `>`+`>` with no
            // intervening whitespace form `<<` and `>>`.  Type parsing never
            // calls this path, so `Vector<Vector<Int>>` is unaffected.
            if (op_kind == TokenKind::Lt || op_kind == TokenKind::Gt)
                && self.is_adjacent_shift(op_kind)
            {
                let (l_bp, r_bp) = shift_binding_power();
                if l_bp < min_bp {
                    break;
                }
                let shift_op = if op_kind == TokenKind::Lt {
                    BinOp::Shl
                } else {
                    BinOp::Shr
                };
                self.advance(); // consume first < or >
                self.advance(); // consume second < or >
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(&rhs.span);
                lhs = Expr::new(
                    self.alloc_expr_id(),
                    ExprKind::Binary {
                        op: shift_op,
                        left: Box::new(lhs),
                        right: Box::new(rhs),
                    },
                    span,
                );
                continue;
            }

            // Check for infix operators
            if let Some((l_bp, r_bp)) = infix_binding_power(op_kind) {
                if l_bp < min_bp {
                    break;
                }

                self.advance(); // consume operator
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(&rhs.span);

                lhs = Expr::new(
                    self.alloc_expr_id(),
                    ExprKind::Binary {
                        op: token_to_binop(op_kind),
                        left: Box::new(lhs),
                        right: Box::new(rhs),
                    },
                    span,
                );
                continue;
            }

            // Check for postfix operators
            if let Some(bp) = postfix_binding_power(op_kind) {
                if bp < min_bp {
                    break;
                }

                let op_token = self.tokens.get(self.pos).unwrap();

                // Calls and indexing are same-line only. Lowercase dot chains
                // may continue across lines to keep multiline method chaining
                // readable.
                if op_token.preceded_by_newline && op_kind != TokenKind::Dot {
                    break;
                }

                // `.Upper` in postfix position is a hard error when the uppercase
                // identifier is *terminal* (not followed by another `.`).
                // An intermediate `.Upper.` is allowed as a qualified type prefix
                // (e.g. `pt.Point.{ ... }` or `mod.Type.Variant`), but a terminal
                // `.Upper` (`.Upper(...)` or `.Upper` at end) is a constructor and
                // must appear in prefix form only.
                if op_kind == TokenKind::Dot {
                    if let Some(next_tok) = self.tokens.get(self.pos + 1) {
                        // `.{` and `.Upper` on a new line start a fresh
                        // expression instead of extending the previous one.
                        if op_token.preceded_by_newline {
                            if next_tok.kind == TokenKind::LBrace {
                                break;
                            }
                            if next_tok.kind == TokenKind::Ident
                                && next_tok.text.starts_with(|c: char| c.is_uppercase())
                            {
                                break;
                            }
                        }

                        if next_tok.kind == TokenKind::Ident
                            && next_tok.text.starts_with(|c: char| c.is_uppercase())
                        {
                            // Same line: if followed by another `.`, it's an intermediate
                            // qualifier (e.g. `pt.Point.{...}`) — allow. Otherwise check
                            // if the base is a type path (has an uppercase segment like
                            // `module.Type`). Type paths may use terminal `.Upper` for
                            // qualified variant constructors. Pure value paths (`x`, `foo.bar`)
                            // remain a hard error.
                            let followed_by_dot = matches!(
                                self.tokens.get(self.pos + 2).map(|t| t.kind),
                                Some(TokenKind::Dot)
                            );
                            if !followed_by_dot && !expr_chain_has_upper_segment(&lhs) {
                                let name = next_tok.text.clone();
                                let span = next_tok.span;
                                return Err(ParseError::new(
                                    ParseErrorKind::ConstructorInPostfix { name },
                                    span,
                                ));
                            }
                        }
                    }
                }

                lhs = self.parse_postfix(lhs)?;
                continue;
            }

            break;
        }

        Ok(lhs)
    }

    fn parse_prefix(&mut self) -> ParseResult<Expr> {
        let token = self
            .peek()
            .ok_or_else(|| ParseError::unexpected_eof(self.eof_span(), vec!["expression"]))?;

        match token.kind {
            // Literals
            TokenKind::IntLit => self.parse_int_literal(),
            TokenKind::FloatLit => self.parse_float_literal(),
            TokenKind::StringLit => self.parse_string_literal(),
            TokenKind::StringStart => self.parse_string_interpolation(),
            TokenKind::True | TokenKind::False => self.parse_bool_literal(),

            // Identifier — if uppercase, greedily consume constructor path Upper(.Upper)*
            TokenKind::Ident => {
                let ident = self.parse_ident()?;
                if let ExprKind::Ident(ref name) = ident.kind {
                    if name.starts_with(|c: char| c.is_uppercase()) {
                        return self.parse_constructor_path(ident);
                    }
                }
                Ok(ident)
            }

            // Unary operators
            TokenKind::Minus | TokenKind::Bang | TokenKind::Tilde => self.parse_unary(),

            // Grouping
            TokenKind::LParen => self.parse_grouped(),

            // Array literal
            TokenKind::LBracket => self.parse_array_literal(),

            // Dot prefix: .Variant or .{ record }
            TokenKind::Dot => self.parse_dot_prefix(),

            // Keywords
            TokenKind::If => self.parse_if(),
            TokenKind::Case => self.parse_case(),
            TokenKind::LBrace => self.parse_block_expr(),
            TokenKind::Fn => self.parse_function_expr(),
            TokenKind::Collect => self.parse_collect(),
            TokenKind::Try => self.parse_try(),
            TokenKind::Return => Err(ParseError::new(
                ParseErrorKind::StatementInExpression {
                    statement: "return",
                    context: None,
                },
                token.span,
            )),
            TokenKind::Break => Err(ParseError::new(
                ParseErrorKind::StatementInExpression {
                    statement: "break",
                    context: None,
                },
                token.span,
            )),
            TokenKind::Continue => Err(ParseError::new(
                ParseErrorKind::StatementInExpression {
                    statement: "continue",
                    context: None,
                },
                token.span,
            )),
            TokenKind::Defer => Err(ParseError::new(
                ParseErrorKind::StatementInExpression {
                    statement: "defer",
                    context: None,
                },
                token.span,
            )),
            TokenKind::For => Err(ParseError::new(
                ParseErrorKind::StatementInExpression {
                    statement: "for",
                    context: None,
                },
                token.span,
            )),

            _ => Err(ParseError::unexpected_token(vec!["expression"], token)),
        }
    }

    fn parse_postfix(&mut self, base: Expr) -> ParseResult<Expr> {
        let kind = self.peek_kind().unwrap();

        match kind {
            TokenKind::LParen => self.parse_call(base),
            TokenKind::Dot => self.parse_field_access(base),
            TokenKind::LBracket => self.parse_index(base),
            _ => unreachable!("parse_postfix called with non-postfix token"),
        }
    }

    // Literal parsing

    fn parse_int_literal(&mut self) -> ParseResult<Expr> {
        let token = self.expect(TokenKind::IntLit)?;
        let value = Self::parse_int_value(&token)?;

        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::Literal(Literal::Int(value)),
            token.span,
        ))
    }

    fn parse_int_value(token: &Token) -> ParseResult<i64> {
        if let Some(hex) = token.text.strip_prefix("0x") {
            i64::from_str_radix(hex, 16).map_err(|_| {
                ParseError::new(
                    ParseErrorKind::UnexpectedToken {
                        expected: vec!["valid hexadecimal literal".to_string()],
                        found: format!("hexadecimal literal out of range for Int: {}", token.text),
                    },
                    token.span,
                )
            })
        } else {
            token.text.parse::<i64>().map_err(|_| {
                ParseError::new(
                    ParseErrorKind::UnexpectedToken {
                        expected: vec!["valid integer literal".to_string()],
                        found: format!("invalid integer: {}", token.text),
                    },
                    token.span,
                )
            })
        }
    }

    fn parse_float_literal(&mut self) -> ParseResult<Expr> {
        let token = self.expect(TokenKind::FloatLit)?;
        let value = token.text.parse::<f64>().map_err(|_| {
            ParseError::new(
                ParseErrorKind::UnexpectedToken {
                    expected: vec!["valid float literal".to_string()],
                    found: format!("invalid float: {}", token.text),
                },
                token.span,
            )
        })?;

        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::Literal(Literal::Float(value)),
            token.span,
        ))
    }

    fn parse_string_literal(&mut self) -> ParseResult<Expr> {
        let token = self.expect(TokenKind::StringLit)?;
        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::Literal(Literal::String(token.text.clone())),
            token.span,
        ))
    }

    fn parse_string_interpolation(&mut self) -> ParseResult<Expr> {
        let start_token = self.expect(TokenKind::StringStart)?;
        let start = start_token.span;
        let mut parts = vec![StringPart::Literal(start_token.text.clone())];

        loop {
            // Parse interpolated expression
            let expr = self.parse_expr()?;
            parts.push(StringPart::Interpolation(Box::new(expr)));

            // Next should be StringContinue or StringEnd
            match self.peek_kind() {
                Some(TokenKind::StringContinue) => {
                    let token = self.advance().unwrap();
                    parts.push(StringPart::Literal(token.text.clone()));
                }
                Some(TokenKind::StringEnd) => {
                    let token = self.advance().unwrap();
                    if !token.text.is_empty() {
                        parts.push(StringPart::Literal(token.text.clone()));
                    }
                    let span = start.merge(&token.span);
                    return Ok(Expr::new(
                        self.alloc_expr_id(),
                        ExprKind::StringInterpolation { parts },
                        span,
                    ));
                }
                _ => {
                    return Err(ParseError::unexpected_token(
                        vec!["string continuation or end"],
                        self.peek().unwrap(),
                    ));
                }
            }
        }
    }

    fn parse_bool_literal(&mut self) -> ParseResult<Expr> {
        let token = self.advance().unwrap();
        let value = token.kind == TokenKind::True;
        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::Literal(Literal::Bool(value)),
            token.span,
        ))
    }

    fn parse_ident(&mut self) -> ParseResult<Expr> {
        let token = self.expect(TokenKind::Ident)?;
        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::Ident(token.text.clone()),
            token.span,
        ))
    }

    /// Greedily extend `base` (an uppercase `Ident`) with any following `.Upper` segments,
    /// forming a constructor path like `FB.Fizz` or `Http.Header.Variant`.
    /// Only uppercase segments are consumed; the first lowercase `.lower` stops here
    /// and is left for the postfix loop.
    fn parse_constructor_path(&mut self, mut base: Expr) -> ParseResult<Expr> {
        loop {
            // Peek: next must be `.` and the token after that an uppercase Ident
            let next_is_dot = self.peek_is(TokenKind::Dot);
            if !next_is_dot {
                break;
            }
            // Peek at the token after the dot
            let after_dot = self.tokens.get(self.pos + 1);
            match after_dot {
                Some(t)
                    if t.kind == TokenKind::Ident
                        && t.text.starts_with(|c: char| c.is_uppercase()) => {}
                _ => break, // lowercase or non-ident — stop, let postfix loop handle it
            }

            // Consume `.` and the uppercase ident
            self.expect(TokenKind::Dot)?;
            let seg = self.expect(TokenKind::Ident)?;
            let span = base.span.merge(&seg.span);
            base = Expr::new(
                self.alloc_expr_id(),
                ExprKind::FieldAccess {
                    base: Box::new(base),
                    field: seg.text.clone(),
                },
                span,
            );
        }
        Ok(base)
    }

    // Operators

    fn parse_unary(&mut self) -> ParseResult<Expr> {
        let op_token = self.advance().unwrap();
        let op = match op_token.kind {
            TokenKind::Minus => UnOp::Neg,
            TokenKind::Bang => UnOp::Not,
            TokenKind::Tilde => UnOp::BitNot,
            _ => unreachable!(),
        };

        let expr = self.parse_expr_bp(prefix_binding_power())?;
        let span = op_token.span.merge(&expr.span);

        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::Unary {
                op,
                expr: Box::new(expr),
            },
            span,
        ))
    }

    fn parse_grouped(&mut self) -> ParseResult<Expr> {
        self.expect(TokenKind::LParen)?;
        let expr = self.parse_expr_in("grouped expression")?;
        self.expect(TokenKind::RParen)?;
        Ok(expr)
    }

    // Postfix operators

    fn parse_call(&mut self, callee: Expr) -> ParseResult<Expr> {
        let start = callee.span;
        self.expect(TokenKind::LParen)?;

        let mut args = Vec::new();
        while !self.peek_is(TokenKind::RParen) && !self.is_eof() {
            args.push(self.parse_expr_in("call argument")?);

            if !self.peek_is(TokenKind::RParen) {
                self.expect(TokenKind::Comma)?;
            }
        }

        let end = self.expect(TokenKind::RParen)?;
        let span = start.merge(&end.span);

        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::Call {
                callee: Box::new(callee),
                args,
            },
            span,
        ))
    }

    fn parse_field_access(&mut self, base: Expr) -> ParseResult<Expr> {
        let start = base.span;
        self.expect(TokenKind::Dot)?;

        // Named record constructor: Type.{ ... } or module.Type.{ ... }
        if self.peek_is(TokenKind::LBrace) {
            if let Some(type_name) = expr_as_type_name(&base) {
                return self.parse_record_literal(Some(type_name), start);
            }
        }

        let field = self.expect(TokenKind::Ident)?;
        let span = start.merge(&field.span);

        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::FieldAccess {
                base: Box::new(base),
                field: field.text.clone(),
            },
            span,
        ))
    }

    fn parse_index(&mut self, base: Expr) -> ParseResult<Expr> {
        let start = base.span;
        self.expect(TokenKind::LBracket)?;
        let index = self.parse_expr()?;
        let end = self.expect(TokenKind::RBracket)?;
        let span = start.merge(&end.span);

        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::Index {
                base: Box::new(base),
                index: Box::new(index),
            },
            span,
        ))
    }

    // Complex expressions

    fn parse_array_literal(&mut self) -> ParseResult<Expr> {
        let start = self.expect(TokenKind::LBracket)?;
        let mut elements = Vec::new();

        while !self.peek_is(TokenKind::RBracket) && !self.is_eof() {
            elements.push(self.parse_expr_in("array element")?);

            if !self.peek_is(TokenKind::RBracket) {
                self.expect(TokenKind::Comma)?;
            }
        }

        let end = self.expect(TokenKind::RBracket)?;
        let span = start.span.merge(&end.span);

        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::Array { elements },
            span,
        ))
    }

    fn parse_dot_prefix(&mut self) -> ParseResult<Expr> {
        let dot = self.expect(TokenKind::Dot)?;

        // Check if it's a record literal or variant
        if self.peek_is(TokenKind::LBrace) {
            // Record literal: .{ ... }
            return self.parse_record_literal(None, dot.span);
        }

        // Variant literal: .Variant or .Variant(...)
        // Variant names must start with an uppercase letter.
        let name = self.expect(TokenKind::Ident)?;
        let variant_name = name.text.clone();

        if variant_name.starts_with(|c: char| c.is_lowercase()) {
            return Err(ParseError::new(
                ParseErrorKind::LowercaseVariant { name: variant_name },
                name.span,
            ));
        }

        if self.peek_is(TokenKind::LParen) {
            // .Variant(fields)
            self.expect(TokenKind::LParen)?;
            let mut fields = Vec::new();

            while !self.peek_is(TokenKind::RParen) && !self.is_eof() {
                fields.push(self.parse_expr()?);

                if !self.peek_is(TokenKind::RParen) {
                    self.expect(TokenKind::Comma)?;
                }
            }

            let end = self.expect(TokenKind::RParen)?;
            let span = dot.span.merge(&end.span);

            Ok(Expr::new(
                self.alloc_expr_id(),
                ExprKind::VariantLit {
                    name: variant_name,
                    fields,
                },
                span,
            ))
        } else {
            // .Variant (no fields)
            let span = dot.span.merge(&name.span);
            Ok(Expr::new(
                self.alloc_expr_id(),
                ExprKind::VariantLit {
                    name: variant_name,
                    fields: Vec::new(),
                },
                span,
            ))
        }
    }

    fn parse_record_literal(&mut self, name: Option<String>, start: Span) -> ParseResult<Expr> {
        self.expect(TokenKind::LBrace)?;
        let mut fields = Vec::new();

        while !self.peek_is(TokenKind::RBrace) && !self.is_eof() {
            let field_name = self.expect(TokenKind::Ident)?;
            let value = if self.consume_if(TokenKind::Colon) {
                self.parse_expr()?
            } else {
                Expr::new(
                    self.alloc_expr_id(),
                    ExprKind::Ident(field_name.text.clone()),
                    field_name.span,
                )
            };

            fields.push((field_name.text.clone(), value));

            if !self.peek_is(TokenKind::RBrace) {
                self.expect(TokenKind::Comma)?;
            }
        }

        let end = self.expect(TokenKind::RBrace)?;
        let span = start.merge(&end.span);

        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::RecordLit { name, fields },
            span,
        ))
    }

    fn parse_if(&mut self) -> ParseResult<Expr> {
        let start = self.expect(TokenKind::If)?;
        let cond = self.parse_expr()?;
        let then_branch = self.parse_block_expr()?;

        let else_branch = if self.consume_if(TokenKind::Else) {
            Some(Box::new(if self.peek_is(TokenKind::If) {
                self.parse_if()?
            } else {
                self.parse_block_expr()?
            }))
        } else {
            None
        };

        let span = if let Some(ref e) = else_branch {
            start.span.merge(&e.span)
        } else {
            start.span.merge(&then_branch.span)
        };

        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::If {
                cond: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch,
            },
            span,
        ))
    }

    fn parse_case(&mut self) -> ParseResult<Expr> {
        let start = self.expect(TokenKind::Case)?;
        let scrutinee = self.parse_expr()?;
        self.expect(TokenKind::LBrace)?;

        let mut arms = Vec::new();
        while !self.peek_is(TokenKind::RBrace) && !self.is_eof() {
            arms.push(self.parse_case_arm()?);

            if self.peek_is(TokenKind::RBrace) {
                self.consume_if(TokenKind::Comma);
            } else {
                self.expect(TokenKind::Comma)?;
            }
        }

        let end = self.expect(TokenKind::RBrace)?;
        let span = start.span.merge(&end.span);

        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::Case {
                scrutinee: Box::new(scrutinee),
                arms,
            },
            span,
        ))
    }

    fn parse_case_arm(&mut self) -> ParseResult<CaseArm> {
        let pattern = self.parse_pattern()?;
        self.expect(TokenKind::FatArrow)?;
        let body = self.parse_expr_in("case arm body")?;
        let span = pattern.span().merge(&body.span);

        Ok(CaseArm {
            pattern,
            body,
            span,
        })
    }

    fn parse_block_expr(&mut self) -> ParseResult<Expr> {
        let block = self.parse_block()?;
        let span = block.span;
        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::Block(block),
            span,
        ))
    }

    fn parse_function_expr(&mut self) -> ParseResult<Expr> {
        let start = self.expect(TokenKind::Fn)?;
        self.expect(TokenKind::LParen)?;

        let mut params = Vec::new();
        while !self.peek_is(TokenKind::RParen) && !self.is_eof() {
            params.push(self.parse_param()?);

            if !self.peek_is(TokenKind::RParen) {
                self.expect(TokenKind::Comma)?;
            }
        }

        self.expect(TokenKind::RParen)?;

        // Optional return type
        let return_type = if !self.peek_is(TokenKind::LBrace) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let body = Box::new(self.parse_block_expr()?);
        let span = start.span.merge(&body.span);

        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::Function(FunctionExpr {
                params,
                return_type,
                body,
                span,
            }),
            span,
        ))
    }

    fn parse_collect(&mut self) -> ParseResult<Expr> {
        let start = self.expect(TokenKind::Collect)?;
        let saved_pos = self.pos;

        if let Ok(pattern) = self.parse_pattern() {
            let index_pattern = if self.peek_is(TokenKind::Comma) {
                self.expect(TokenKind::Comma)?;
                Some(self.parse_pattern()?)
            } else {
                None
            };
            if self.peek_is(TokenKind::In) {
                self.expect(TokenKind::In)?;
                let iter = self.parse_expr()?;
                let body = self.parse_block_expr()?;
                let span = start.span.merge(&body.span);
                return Ok(Expr::new(
                    self.alloc_expr_id(),
                    ExprKind::Collect {
                        pattern,
                        index_pattern,
                        iter: Box::new(iter),
                        body: Box::new(body),
                    },
                    span,
                ));
            }
        }

        // Backtrack: collect cond { body }
        self.pos = saved_pos;
        let cond = self.parse_expr()?;
        let body = self.parse_block_expr()?;
        let span = start.span.merge(&body.span);
        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::CollectWhile {
                cond: Box::new(cond),
                body: Box::new(body),
            },
            span,
        ))
    }

    fn parse_try(&mut self) -> ParseResult<Expr> {
        let start = self.expect(TokenKind::Try)?;
        let expr = self.parse_expr_bp(prefix_binding_power())?;
        let span = start.span.merge(&expr.span);

        Ok(Expr::new(
            self.alloc_expr_id(),
            ExprKind::Try {
                expr: Box::new(expr),
            },
            span,
        ))
    }

    // Pattern parsing with full variant destructuring support
    fn parse_pattern(&mut self) -> ParseResult<Pattern> {
        match self.peek_kind() {
            Some(TokenKind::Dot) => {
                // Anonymous variant pattern: .Some(x), .Node(val, left, right), .Unit
                let start = self.expect(TokenKind::Dot)?;
                let name_token = self.expect(TokenKind::Ident)?;
                let name = name_token.text.clone();

                // Optional arguments: .Some(x) vs .None
                let fields = if self.peek_is(TokenKind::LParen) {
                    self.expect(TokenKind::LParen)?;
                    let mut fields = Vec::new();

                    while !self.peek_is(TokenKind::RParen) && !self.is_eof() {
                        fields.push(self.parse_pattern()?);

                        if !self.peek_is(TokenKind::RParen) {
                            self.expect(TokenKind::Comma)?;
                        }
                    }

                    let rparen = self.expect(TokenKind::RParen)?;
                    let span = start.span.merge(&rparen.span);

                    return Ok(Pattern::Variant {
                        type_name: None,
                        name,
                        fields,
                        span,
                    });
                } else {
                    Vec::new()
                };

                let span = start.span.merge(&name_token.span);
                Ok(Pattern::Variant {
                    type_name: None,
                    name,
                    fields,
                    span,
                })
            }
            Some(TokenKind::Ident) => {
                let token = self.advance().unwrap();
                if token.text == "_" {
                    Ok(Pattern::Wildcard(token.span))
                } else if token.text.starts_with(|c: char| c.is_uppercase())
                    && self.peek_is(TokenKind::Dot)
                {
                    // Qualified variant pattern: TypeName.Variant or TypeName.Variant(fields)
                    let type_name = token.text.clone();
                    self.expect(TokenKind::Dot)?;
                    let variant_token = self.expect(TokenKind::Ident)?;
                    let name = variant_token.text.clone();

                    let (fields, span) = if self.peek_is(TokenKind::LParen) {
                        self.expect(TokenKind::LParen)?;
                        let mut fields = Vec::new();
                        while !self.peek_is(TokenKind::RParen) && !self.is_eof() {
                            fields.push(self.parse_pattern()?);
                            if !self.peek_is(TokenKind::RParen) {
                                self.expect(TokenKind::Comma)?;
                            }
                        }
                        let rparen = self.expect(TokenKind::RParen)?;
                        let span = token.span.merge(&rparen.span);
                        (fields, span)
                    } else {
                        let span = token.span.merge(&variant_token.span);
                        (Vec::new(), span)
                    };

                    Ok(Pattern::Variant {
                        type_name: Some(type_name),
                        name,
                        fields,
                        span,
                    })
                } else if token.text.starts_with(|c: char| c.is_uppercase()) {
                    return Err(ParseError::new(
                        ParseErrorKind::CaseViolation {
                            kind: "binding name",
                            name: token.text.clone(),
                            expected: "a lowercase",
                        },
                        token.span,
                    ));
                } else {
                    Ok(Pattern::Ident(token.text.clone(), token.span))
                }
            }
            Some(TokenKind::IntLit) => {
                let token = self.advance().unwrap();
                let value = Self::parse_int_value(&token)?;
                Ok(Pattern::Literal(Literal::Int(value), token.span))
            }
            Some(TokenKind::True) => {
                let token = self.advance().unwrap();
                Ok(Pattern::Literal(Literal::Bool(true), token.span))
            }
            Some(TokenKind::False) => {
                let token = self.advance().unwrap();
                Ok(Pattern::Literal(Literal::Bool(false), token.span))
            }
            Some(TokenKind::StringLit) => {
                let token = self.advance().unwrap();
                Ok(Pattern::Literal(
                    Literal::String(token.text.clone()),
                    token.span,
                ))
            }
            _ => {
                let span = self
                    .peek()
                    .map(|t| t.span)
                    .unwrap_or_else(|| self.eof_span());
                Err(ParseError::new(
                    ParseErrorKind::UnexpectedToken {
                        expected: vec![
                            "pattern (identifier, wildcard, literal, or .Variant)".to_string(),
                        ],
                        found: self
                            .peek()
                            .map(|t| t.kind.name().to_string())
                            .unwrap_or_else(|| "EOF".to_string()),
                    },
                    span,
                ))
            }
        }
    }

    fn parse_block(&mut self) -> ParseResult<Block> {
        let start = self.expect(TokenKind::LBrace)?;
        let mut stmts = Vec::new();

        while !self.peek_is(TokenKind::RBrace) && !self.is_eof() {
            stmts.push(self.parse_stmt()?);
        }

        let end = self.expect(TokenKind::RBrace)?;
        let span = start.span.merge(&end.span);

        Ok(Block { stmts, span })
    }

    fn parse_stmt(&mut self) -> ParseResult<Stmt> {
        match self.peek_kind() {
            Some(TokenKind::For) => self.parse_for_stmt(),
            Some(TokenKind::Break) => self.parse_break_stmt(),
            Some(TokenKind::Continue) => self.parse_continue_stmt(),
            Some(TokenKind::Return) => self.parse_return_stmt(),
            Some(TokenKind::Defer) => self.parse_defer_stmt(),
            _ => {
                // Check if this is a let binding by looking ahead
                if self.is_let_binding() {
                    self.parse_let_stmt(false)
                } else {
                    // Otherwise it's an expression statement
                    let expr = self.parse_expr()?;
                    Ok(Stmt::Expr(expr))
                }
            }
        }
    }

    fn is_let_binding(&self) -> bool {
        // A let binding has the form: pattern := expr or pattern: Type = expr
        // We need to look ahead to see if there's a := or : after the pattern
        // For now, we'll use a simple heuristic: if we see an identifier followed by := or :
        if let Some(TokenKind::Ident) = self.peek_kind() {
            if let Some(next) = self.tokens.get(self.pos + 1) {
                matches!(next.kind, TokenKind::ColonEq | TokenKind::Colon)
            } else {
                false
            }
        } else {
            false
        }
    }

    fn parse_let_stmt(&mut self, is_pub: bool) -> ParseResult<Stmt> {
        let pattern = self.parse_pattern()?;

        // Enforce lowercase-initial for value binding identifiers
        if let Pattern::Ident(ref name, span) = pattern {
            if name.starts_with(|c: char| c.is_uppercase()) {
                return Err(ParseError::new(
                    ParseErrorKind::CaseViolation {
                        kind: "value binding",
                        name: name.clone(),
                        expected: "a lowercase",
                    },
                    span,
                ));
            }
        }

        let (ty, value) = if self.peek_is(TokenKind::ColonEq) {
            // x := expr (type inferred)
            self.expect(TokenKind::ColonEq)?;
            let value = self.parse_expr()?;
            (None, value)
        } else if self.peek_is(TokenKind::Colon) {
            // x: Type = expr (explicit type)
            self.expect(TokenKind::Colon)?;
            let ty = self.parse_type()?;
            self.expect(TokenKind::Eq)?;
            let value = self.parse_expr()?;
            (Some(ty), value)
        } else {
            return Err(ParseError::unexpected_token(
                vec![":=", ":"],
                self.peek().unwrap(),
            ));
        };

        let span = pattern.span().merge(&value.span);

        Ok(Stmt::Let {
            pattern,
            ty,
            value,
            is_pub,
            doc: None,
            span,
        })
    }

    fn parse_for_stmt(&mut self) -> ParseResult<Stmt> {
        let start = self.expect(TokenKind::For)?;

        // Try to parse as "for pattern in iter" or "for pattern, index in iter"
        // If that fails, parse as "for cond"

        // Save position to potentially backtrack
        let saved_pos = self.pos;

        // Try iterator form first
        if let Ok(pattern) = self.parse_pattern() {
            // Check for optional index: "for x, i in iter"
            let index_pattern = if self.peek_is(TokenKind::Comma) {
                self.expect(TokenKind::Comma)?;
                Some(self.parse_pattern()?)
            } else {
                None
            };

            if self.peek_is(TokenKind::In) {
                // Iterator form: for pattern in expr { body }
                self.expect(TokenKind::In)?;
                let iter = self.parse_expr()?;
                let body = self.parse_block()?;
                let span = start.span.merge(&body.span);

                return Ok(Stmt::For {
                    pattern,
                    index_pattern,
                    iter,
                    body,
                    span,
                });
            }
        }

        // Backtrack and parse as condition form: for cond { body }
        self.pos = saved_pos;
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        let span = start.span.merge(&body.span);

        Ok(Stmt::ForCond { cond, body, span })
    }

    fn parse_break_stmt(&mut self) -> ParseResult<Stmt> {
        let start = self.expect(TokenKind::Break)?;

        // Optional break value
        let value = if !self.peek_is(TokenKind::RBrace)
            && !self.is_eof()
            && !matches!(
                self.peek_kind(),
                Some(TokenKind::Break | TokenKind::Continue | TokenKind::Return | TokenKind::For)
            ) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        let span = if let Some(ref v) = value {
            start.span.merge(&v.span)
        } else {
            start.span
        };

        Ok(Stmt::Break { value, span })
    }

    fn parse_continue_stmt(&mut self) -> ParseResult<Stmt> {
        let start = self.expect(TokenKind::Continue)?;
        Ok(Stmt::Continue { span: start.span })
    }

    fn parse_defer_stmt(&mut self) -> ParseResult<Stmt> {
        let start = self.expect(TokenKind::Defer)?;
        let expr = self.parse_expr()?;
        let span = start.span.merge(&expr.span);
        Ok(Stmt::Defer { expr, span })
    }

    fn parse_return_stmt(&mut self) -> ParseResult<Stmt> {
        let start = self.expect(TokenKind::Return)?;

        // Optional return value
        let value = if !self.peek_is(TokenKind::RBrace)
            && !self.is_eof()
            && !matches!(
                self.peek_kind(),
                Some(TokenKind::Break | TokenKind::Continue | TokenKind::Return | TokenKind::For)
            ) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        let span = if let Some(ref v) = value {
            start.span.merge(&v.span)
        } else {
            start.span
        };

        Ok(Stmt::Return { value, span })
    }

    fn parse_type(&mut self) -> ParseResult<Type> {
        // !E shorthand: Result<Void, E>
        if self.peek_is(TokenKind::Bang) {
            let bang = self.advance().unwrap();
            let err_ty = self.parse_type_base()?;
            let span = bang.span.merge(&err_ty.span());
            let void_ty = Type::Named {
                name: "Void".to_string(),
                args: vec![],
                span: bang.span,
            };
            return Ok(Type::Named {
                name: "Result".to_string(),
                args: vec![void_ty, err_ty],
                span,
            });
        }

        let base = self.parse_type_base()?;

        // T? sugar for Option<T>
        let base = if self.peek_is(TokenKind::Question) {
            let q = self.advance().unwrap();
            let span = base.span().merge(&q.span);
            Type::Named {
                name: "Option".to_string(),
                args: vec![base],
                span,
            }
        } else {
            base
        };

        // T!E or T?!E sugar for Result<T, E>
        if self.peek_is(TokenKind::Bang) {
            self.advance();
            let err_ty = self.parse_type_base()?;
            let span = base.span().merge(&err_ty.span());
            return Ok(Type::Named {
                name: "Result".to_string(),
                args: vec![base, err_ty],
                span,
            });
        }

        Ok(base)
    }

    fn parse_type_base(&mut self) -> ParseResult<Type> {
        // Check for function type: fn(T1, T2) RetType
        if self.peek_is(TokenKind::Fn) {
            let start = self.expect(TokenKind::Fn)?;
            self.expect(TokenKind::LParen)?;

            let mut params = Vec::new();
            while !self.peek_is(TokenKind::RParen) && !self.is_eof() {
                params.push(self.parse_type()?);

                if !self.peek_is(TokenKind::RParen) {
                    self.expect(TokenKind::Comma)?;
                }
            }

            let rparen = self.expect(TokenKind::RParen)?;

            // Return type (required for function types)
            let return_type = if !matches!(
                self.peek_kind(),
                Some(
                    TokenKind::Comma
                        | TokenKind::RBrace
                        | TokenKind::RParen
                        | TokenKind::Gt
                        | TokenKind::Question
                )
            ) {
                Box::new(self.parse_type()?)
            } else {
                // Default to Void if no return type specified
                let void_span = rparen.span;
                Box::new(Type::Named {
                    name: "Void".to_string(),
                    args: Vec::new(),
                    span: void_span,
                })
            };

            let span = start.span.merge(&return_type.span());

            Ok(Type::Function {
                params,
                ret: return_type,
                span,
            })
        } else {
            // Named type: Ident or Ident<T1, T2> or module.Type
            let name_token = self.expect(TokenKind::Ident)?;
            let mut name = name_token.text.clone();
            let mut end_span = name_token.span;

            // Support qualified type names: module.Type or module.sub.Type
            // Stop before module.{ (record literal) or module.<T> (type arg list on base name)
            while self.peek_is(TokenKind::Dot)
                && matches!(
                    self.tokens.get(self.pos + 1).map(|t| t.kind),
                    Some(TokenKind::Ident)
                )
            {
                self.advance(); // consume dot
                let type_seg = self.expect(TokenKind::Ident)?;
                name = format!("{}.{}", name, type_seg.text);
                end_span = type_seg.span;
            }

            // Optional type arguments: <T1, T2>
            let args = if self.peek_is(TokenKind::Lt) {
                self.expect(TokenKind::Lt)?;
                let mut args = Vec::new();

                while !self.peek_is(TokenKind::Gt) && !self.is_eof() {
                    args.push(self.parse_type()?);

                    if !self.peek_is(TokenKind::Gt) {
                        self.expect(TokenKind::Comma)?;
                    }
                }

                let gt = self.expect(TokenKind::Gt)?;
                end_span = gt.span;
                args
            } else {
                Vec::new()
            };

            let span = name_token.span.merge(&end_span);

            Ok(Type::Named { name, args, span })
        }
    }

    fn parse_param(&mut self) -> ParseResult<Param> {
        let name_token = self.expect(TokenKind::Ident)?;
        let name = name_token.text.clone();
        let start_span = name_token.span;

        if name.starts_with(|c: char| c.is_uppercase()) {
            return Err(ParseError::new(
                ParseErrorKind::CaseViolation {
                    kind: "parameter name",
                    name,
                    expected: "a lowercase",
                },
                start_span,
            ));
        }

        // Optional type annotation
        let ty = if self.peek_is(TokenKind::Colon) {
            self.expect(TokenKind::Colon)?;
            Some(self.parse_type()?)
        } else {
            None
        };

        let span = if let Some(ref t) = ty {
            start_span.merge(&t.span())
        } else {
            start_span
        };

        Ok(Param { name, ty, span })
    }
}

// Helper for pattern spans
impl Pattern {
    fn span(&self) -> Span {
        match self {
            Pattern::Wildcard(s) => *s,
            Pattern::Ident(_, s) => *s,
            Pattern::Literal(_, s) => *s,
            Pattern::Variant { span, .. } => *span,
        }
    }
}

// Operator precedence and binding power

fn infix_binding_power(op: TokenKind) -> Option<(u8, u8)> {
    use TokenKind::*;
    Some(match op {
        // Assignment (right-associative)
        Eq | ColonEq => (2, 1),
        // Range (lower precedence than assignment)
        DotDot => (1, 3),
        // Logical OR
        Or => (3, 4),
        // Logical AND
        And => (5, 6),
        // Bitwise OR
        Pipe => (7, 8),
        // Bitwise XOR
        Caret => (9, 10),
        // Bitwise AND
        Amp => (11, 12),
        // Equality
        EqEq | BangEq => (13, 14),
        // Comparison
        Lt | LtEq | Gt | GtEq => (15, 16),
        // Shift (handled specially via adjacent-token combining, but
        // the binding power is looked up with a synthetic token kind)
        // Additive
        Plus | Minus => (19, 20),
        // Multiplicative
        Star | Slash | Percent => (21, 22),
        _ => return None,
    })
}

/// Binding power for shift operators (`<<`, `>>`).
/// Called when the parser detects adjacent `<`+`<` or `>`+`>`.
fn shift_binding_power() -> (u8, u8) {
    (17, 18)
}

fn postfix_binding_power(op: TokenKind) -> Option<u8> {
    use TokenKind::*;
    Some(match op {
        LParen | Dot | LBracket => 23,
        _ => return None,
    })
}

fn prefix_binding_power() -> u8 {
    22 // Same as multiplicative, higher than additive
}

/// Check if an Ident/FieldAccess chain contains at least one PascalCase segment.
/// Used to distinguish type paths (e.g. `module.Type`) from value paths (`x`, `foo.bar`).
fn expr_chain_has_upper_segment(expr: &crate::syntax::ast::Expr) -> bool {
    use crate::syntax::ast::ExprKind;
    match &expr.kind {
        ExprKind::Ident(name) => name.starts_with(|c: char| c.is_uppercase()),
        ExprKind::FieldAccess { base, field } => {
            field.starts_with(|c: char| c.is_uppercase()) || expr_chain_has_upper_segment(base)
        }
        _ => false,
    }
}

/// Extract a dotted type name from an expression for use in named record literals.
/// `Ident("Vec2")` → `Some("Vec2")`
/// `FieldAccess { base: Ident("pt"), field: "Point" }` → `Some("pt.Point")`
fn expr_as_type_name(expr: &crate::syntax::ast::Expr) -> Option<String> {
    use crate::syntax::ast::ExprKind;
    match &expr.kind {
        ExprKind::Ident(name) => Some(name.clone()),
        ExprKind::FieldAccess { base, field } => {
            expr_as_type_name(base).map(|prefix| format!("{}.{}", prefix, field))
        }
        _ => None,
    }
}

fn token_to_binop(kind: TokenKind) -> BinOp {
    use TokenKind::*;
    match kind {
        Plus => BinOp::Add,
        Minus => BinOp::Sub,
        Star => BinOp::Mul,
        Slash => BinOp::Div,
        Percent => BinOp::Mod,
        EqEq => BinOp::Eq,
        BangEq => BinOp::Ne,
        Lt => BinOp::Lt,
        LtEq => BinOp::Le,
        Gt => BinOp::Gt,
        GtEq => BinOp::Ge,
        Amp => BinOp::BitAnd,
        Pipe => BinOp::BitOr,
        Caret => BinOp::BitXor,
        And => BinOp::And,
        Or => BinOp::Or,
        Eq => BinOp::Assign,
        DotDot => BinOp::Range,
        _ => unreachable!("token_to_binop called with non-binary-op token"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::lexer::Lexer;

    fn parse_expr(source: &str) -> ParseResult<Expr> {
        let file_id = FileId(0);
        let tokens = Lexer::lex(source, file_id).unwrap();
        let mut parser = Parser::new(tokens, file_id);
        parser.parse_expr()
    }

    #[test]
    fn test_parse_int_literal() {
        let expr = parse_expr("42").unwrap();
        assert!(matches!(expr.kind, ExprKind::Literal(Literal::Int(42))));
    }

    #[test]
    fn test_parse_hex_literal() {
        let expr = parse_expr("0xFF").unwrap();
        assert!(matches!(expr.kind, ExprKind::Literal(Literal::Int(255))));

        let expr = parse_expr("0x10").unwrap();
        assert!(matches!(expr.kind, ExprKind::Literal(Literal::Int(16))));

        let expr = parse_expr("0x0").unwrap();
        assert!(matches!(expr.kind, ExprKind::Literal(Literal::Int(0))));

        let expr = parse_expr("0x7FFFFFFFFFFFFFFF").unwrap();
        assert!(matches!(
            expr.kind,
            ExprKind::Literal(Literal::Int(i64::MAX))
        ));
    }

    #[test]
    fn test_parse_hex_overflow() {
        let result = parse_expr("0x8000000000000000");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_hex_in_binary_expr() {
        let expr = parse_expr("0xFF + 1").unwrap();
        match expr.kind {
            ExprKind::Binary {
                op: BinOp::Add,
                left,
                right,
            } => {
                assert!(matches!(left.kind, ExprKind::Literal(Literal::Int(255))));
                assert!(matches!(right.kind, ExprKind::Literal(Literal::Int(1))));
            }
            _ => panic!("Expected binary expression"),
        }
    }

    #[test]
    fn test_parse_negated_hex() {
        let expr = parse_expr("-0x1").unwrap();
        match expr.kind {
            ExprKind::Unary { op, expr, .. } => {
                assert_eq!(op, UnOp::Neg);
                assert!(matches!(expr.kind, ExprKind::Literal(Literal::Int(1))));
            }
            _ => panic!("Expected unary expression"),
        }
    }

    #[test]
    fn test_parse_binary_expr() {
        let expr = parse_expr("1 + 2").unwrap();
        match expr.kind {
            ExprKind::Binary { op, .. } => assert_eq!(op, BinOp::Add),
            _ => panic!("Expected binary expression"),
        }
    }

    #[test]
    fn test_parse_precedence() {
        let expr = parse_expr("1 + 2 * 3").unwrap();
        // Should parse as 1 + (2 * 3)
        match expr.kind {
            ExprKind::Binary {
                op: BinOp::Add,
                left,
                right,
            } => {
                assert!(matches!(left.kind, ExprKind::Literal(Literal::Int(1))));
                assert!(matches!(
                    right.kind,
                    ExprKind::Binary { op: BinOp::Mul, .. }
                ));
            }
            _ => panic!("Expected binary addition"),
        }
    }

    #[test]
    fn test_parse_call() {
        let expr = parse_expr("foo(1, 2)").unwrap();
        match expr.kind {
            ExprKind::Call { args, .. } => assert_eq!(args.len(), 2),
            _ => panic!("Expected call expression"),
        }
    }

    #[test]
    fn test_parse_field_access() {
        let expr = parse_expr("point.x").unwrap();
        match expr.kind {
            ExprKind::FieldAccess { field, .. } => assert_eq!(field, "x"),
            _ => panic!("Expected field access"),
        }
    }

    #[test]
    fn test_call_and_index_require_same_line() {
        let source = r#"fn f() {
foo := "asdf"
[1, 22, 3]
bar
(1)
}"#;
        let file = parse_source(source).unwrap();

        let body = match &file.items[0] {
            Item::Function(func) => &func.body,
            _ => panic!("Expected function item"),
        };

        assert_eq!(body.stmts.len(), 4);

        match &body.stmts[0] {
            Stmt::Let { .. } => {}
            _ => panic!("Expected first statement to be let binding"),
        }
        match &body.stmts[1] {
            Stmt::Expr(expr) => assert!(matches!(expr.kind, ExprKind::Array { .. })),
            _ => panic!("Expected second statement to be array literal"),
        }
        match &body.stmts[2] {
            Stmt::Expr(expr) => {
                assert!(matches!(expr.kind, ExprKind::Ident(ref name) if name == "bar"))
            }
            _ => panic!("Expected third statement to be identifier expression"),
        }
        match &body.stmts[3] {
            Stmt::Expr(expr) => assert!(matches!(expr.kind, ExprKind::Literal(Literal::Int(1)))),
            _ => panic!("Expected fourth statement to be grouped integer expression"),
        }
    }

    #[test]
    fn test_lowercase_method_chain_can_cross_newline() {
        let source = r#"fn f() {
foo
.bar()
}"#;

        let file = parse_source(source).unwrap();
        let body = match &file.items[0] {
            Item::Function(func) => &func.body,
            _ => panic!("Expected function item"),
        };

        assert_eq!(body.stmts.len(), 1);
        match &body.stmts[0] {
            Stmt::Expr(expr) => match &expr.kind {
                ExprKind::Call { callee, args } => {
                    assert!(args.is_empty());
                    assert!(matches!(
                        callee.kind,
                        ExprKind::FieldAccess { ref field, .. } if field == "bar"
                    ));
                }
                _ => panic!("Expected method call expression"),
            },
            _ => panic!("Expected expression statement"),
        }
    }

    #[test]
    fn test_parse_array() {
        let expr = parse_expr("[1, 2, 3]").unwrap();
        match expr.kind {
            ExprKind::Array { elements } => assert_eq!(elements.len(), 3),
            _ => panic!("Expected array literal"),
        }
    }

    #[test]
    fn test_parse_record_literal_field_punning_anonymous() {
        let expr = parse_expr(".{ x, y: 2 }").unwrap();
        match expr.kind {
            ExprKind::RecordLit { name, fields } => {
                assert_eq!(name, None);
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].0, "x");
                assert!(matches!(fields[0].1.kind, ExprKind::Ident(ref ident) if ident == "x"));
                assert_eq!(fields[1].0, "y");
                assert!(matches!(
                    fields[1].1.kind,
                    ExprKind::Literal(Literal::Int(2))
                ));
            }
            _ => panic!("Expected record literal"),
        }
    }

    #[test]
    fn test_parse_record_literal_field_punning_named_with_trailing_comma() {
        let expr = parse_expr("Point.{ x, y, }").unwrap();
        match expr.kind {
            ExprKind::RecordLit { name, fields } => {
                assert_eq!(name, Some("Point".to_string()));
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].0, "x");
                assert!(matches!(fields[0].1.kind, ExprKind::Ident(ref ident) if ident == "x"));
                assert_eq!(fields[1].0, "y");
                assert!(matches!(fields[1].1.kind, ExprKind::Ident(ref ident) if ident == "y"));
            }
            _ => panic!("Expected record literal"),
        }
    }

    fn parse_source(source: &str) -> ParseResult<SourceFile> {
        let file_id = FileId(0);
        let tokens = Lexer::lex(source, file_id).unwrap();
        let mut parser = Parser::new(tokens, file_id);
        parser.parse_source_file()
    }

    fn get_import(source: &str) -> ImportDecl {
        let sf = parse_source(source).unwrap();
        match &sf.items[0] {
            Item::Import(decl) => decl.clone(),
            other => panic!("Expected Import, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_relative_import_single() {
        let decl = get_import("use .foo");
        assert!(decl.is_relative);
        assert!(!decl.is_stdlib);
        assert_eq!(decl.module_path, vec!["foo"]);
        assert_eq!(decl.alias, None);
    }

    #[test]
    fn test_parse_relative_import_nested() {
        let decl = get_import("use .foo.bar");
        assert!(decl.is_relative);
        assert_eq!(decl.module_path, vec!["foo", "bar"]);
    }

    #[test]
    fn test_parse_relative_import_with_alias() {
        let decl = get_import("use .foo as f");
        assert!(decl.is_relative);
        assert_eq!(decl.module_path, vec!["foo"]);
        assert_eq!(decl.alias, Some("f".to_string()));
    }

    #[test]
    fn test_parse_relative_import_does_not_affect_absolute() {
        let decl = get_import("use foo.bar");
        assert!(!decl.is_relative);
        assert!(!decl.is_stdlib);
        assert_eq!(decl.module_path, vec!["foo", "bar"]);
    }

    #[test]
    fn test_parse_relative_import_does_not_affect_stdlib() {
        let decl = get_import("use @std.fs");
        assert!(!decl.is_relative);
        assert!(decl.is_stdlib);
        assert_eq!(decl.module_path, vec!["std", "fs"]);
    }

    #[test]
    fn test_parse_relative_import_reject_bare_dot() {
        // `use .` with no identifier should fail
        let result = parse_source("use .");
        assert!(result.is_err(), "Expected parse error for `use .`");
    }

    #[test]
    fn test_parse_relative_import_reject_double_dot() {
        // `use ..foo` — parent traversal not supported in MVP
        let result = parse_source("use ..foo");
        assert!(result.is_err(), "Expected parse error for `use ..foo`");
    }

    // ── Destructuring import tests ──────────────────────────────────────

    #[test]
    fn test_parse_destructuring_value_imports() {
        let decl = get_import("use foo.bar.{translate, scale}");
        assert_eq!(decl.module_path, vec!["foo", "bar"]);
        assert_eq!(decl.alias, None);
        let items = decl.items.unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0],
            ImportItem::Value {
                name: "translate".into(),
                alias: None,
                span: items[0].span()
            }
        );
        assert_eq!(
            items[1],
            ImportItem::Value {
                name: "scale".into(),
                alias: None,
                span: items[1].span()
            }
        );
    }

    #[test]
    fn test_parse_destructuring_type_imports() {
        let decl = get_import("use foo.bar.{Vec2, Transform}");
        assert_eq!(decl.module_path, vec!["foo", "bar"]);
        let items = decl.items.unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0],
            ImportItem::Type {
                name: "Vec2".into(),
                alias: None,
                span: items[0].span()
            }
        );
        assert_eq!(
            items[1],
            ImportItem::Type {
                name: "Transform".into(),
                alias: None,
                span: items[1].span()
            }
        );
    }

    #[test]
    fn test_parse_destructuring_mixed_imports() {
        let decl = get_import("use foo.bar.{translate, Vec2}");
        assert_eq!(decl.module_path, vec!["foo", "bar"]);
        let items = decl.items.unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0],
            ImportItem::Value {
                name: "translate".into(),
                alias: None,
                span: items[0].span()
            }
        );
        assert_eq!(
            items[1],
            ImportItem::Type {
                name: "Vec2".into(),
                alias: None,
                span: items[1].span()
            }
        );
    }

    #[test]
    fn test_parse_destructuring_with_aliases() {
        let decl = get_import("use foo.bar.{translate as tr, Vec2 as V2}");
        assert_eq!(decl.module_path, vec!["foo", "bar"]);
        let items = decl.items.unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0],
            ImportItem::Value {
                name: "translate".into(),
                alias: Some("tr".into()),
                span: items[0].span(),
            }
        );
        assert_eq!(
            items[1],
            ImportItem::Type {
                name: "Vec2".into(),
                alias: Some("V2".into()),
                span: items[1].span(),
            }
        );
    }

    #[test]
    fn test_parse_destructuring_single_item() {
        let decl = get_import("use foo.bar.{translate}");
        assert_eq!(decl.module_path, vec!["foo", "bar"]);
        let items = decl.items.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0],
            ImportItem::Value {
                name: "translate".into(),
                alias: None,
                span: items[0].span()
            }
        );
    }

    #[test]
    fn test_parse_destructuring_preserves_module_name() {
        // module_name() should still return last path segment
        let decl = get_import("use foo.bar.{translate}");
        assert_eq!(decl.module_name(), "bar");
    }

    #[test]
    fn test_parse_destructuring_with_stdlib() {
        let decl = get_import("use @std.fs.{read, write}");
        assert!(decl.is_stdlib);
        assert_eq!(decl.module_path, vec!["std", "fs"]);
        let items = decl.items.unwrap();
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn test_parse_destructuring_with_relative() {
        let decl = get_import("use .foo.{bar}");
        assert!(decl.is_relative);
        assert_eq!(decl.module_path, vec!["foo"]);
        let items = decl.items.unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_parse_destructuring_reject_combined_alias_and_items() {
        // `use foo.bar as baz.{x}` is not supported
        let result = parse_source("use foo.bar as baz.{x}");
        assert!(
            result.is_err(),
            "Expected parse error for combined alias + items"
        );
    }

    #[test]
    fn test_parse_plain_import_still_works() {
        // Ensure existing forms are unaffected
        let decl = get_import("use foo.bar");
        assert_eq!(decl.module_path, vec!["foo", "bar"]);
        assert_eq!(decl.alias, None);
        assert!(decl.items.is_none());
    }

    #[test]
    fn test_parse_alias_import_still_works() {
        let decl = get_import("use foo.bar as baz");
        assert_eq!(decl.module_path, vec!["foo", "bar"]);
        assert_eq!(decl.alias, Some("baz".into()));
        assert!(decl.items.is_none());
    }

    #[test]
    fn test_parse_destructuring_trailing_comma() {
        // Trailing comma should be accepted
        let decl = get_import("use foo.bar.{translate, scale,}");
        let items = decl.items.unwrap();
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn test_parse_destructuring_reject_empty_braces() {
        let result = parse_source("use foo.bar.{}");
        assert!(
            result.is_err(),
            "Expected parse error for empty import list"
        );
    }

    #[test]
    fn test_parse_range_expr() {
        // Simple `m..n` → Binary { op: Range, left: m, right: n }
        let expr = parse_expr("1..10").unwrap();
        match expr.kind {
            ExprKind::Binary {
                op: BinOp::Range,
                left,
                right,
            } => {
                assert!(matches!(left.kind, ExprKind::Literal(Literal::Int(1))));
                assert!(matches!(right.kind, ExprKind::Literal(Literal::Int(10))));
            }
            _ => panic!("Expected range expression"),
        }
    }

    #[test]
    fn test_parse_range_lower_than_arithmetic() {
        // `1 + 2..3 + 4` → (1+2)..(3+4): arithmetic binds tighter than range
        let expr = parse_expr("1 + 2..3 + 4").unwrap();
        match expr.kind {
            ExprKind::Binary {
                op: BinOp::Range,
                left,
                right,
            } => {
                assert!(matches!(left.kind, ExprKind::Binary { op: BinOp::Add, .. }));
                assert!(matches!(
                    right.kind,
                    ExprKind::Binary { op: BinOp::Add, .. }
                ));
            }
            _ => panic!("Expected range expression with arithmetic operands"),
        }
    }

    #[test]
    fn test_parse_range_in_assign_rhs() {
        // `a = m..n` → assign(a, range(m, n)): range is consumed inside the assign RHS
        let expr = parse_expr("a = 1..10").unwrap();
        match expr.kind {
            ExprKind::Binary {
                op: BinOp::Assign,
                right,
                ..
            } => {
                assert!(matches!(
                    right.kind,
                    ExprKind::Binary {
                        op: BinOp::Range,
                        ..
                    }
                ));
            }
            _ => panic!("Expected assignment with range on the right"),
        }
    }
}
