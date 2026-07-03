// Lexer, Parser, AST - Stage 1

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod pretty;
pub mod span;
pub mod tokens;

use anyhow::Result;
use ast::SourceFile;
use span::FileRegistry;

/// Parse a Twinkle source file into an AST
/// Returns both the AST and the FileRegistry for error formatting
pub fn parse_source(source: &str, file_name: &str) -> Result<(SourceFile, FileRegistry)> {
    let mut registry = FileRegistry::new();
    let file_id = registry.add_file(file_name.to_string(), source.to_string());

    let tokens =
        lexer::Lexer::lex(source, file_id).map_err(|e| format_lexer_error(&registry, e))?;

    let mut parser = parser::Parser::new(tokens, file_id);

    // Parse full source file with all items
    let mut source_file = parser
        .parse_source_file()
        .map_err(|e| format_parse_error(&registry, e))?;
    attach_top_level_doc_comments(&mut source_file, source);

    Ok((source_file, registry))
}

fn attach_top_level_doc_comments(source_file: &mut SourceFile, source: &str) {
    let line_starts = compute_line_starts(source);

    // Top-level items are parsed in source order, so their start offsets are
    // non-decreasing. A forward cursor converts each char offset to a byte
    // offset in a single amortized linear pass over the file, instead of
    // rescanning from the start per item (which was O(items * filesize)).
    let mut cursor = CharByteCursor::new(source);

    for item in &mut source_file.items {
        let (doc_slot, start_char): (&mut Option<String>, u32) = match item {
            ast::Item::Function(decl) => (&mut decl.doc, decl.span.start),
            ast::Item::TypeDecl(decl) => (&mut decl.doc, decl.span.start),
            ast::Item::Stmt(ast::Stmt::Let {
                is_pub: true,
                span,
                doc,
                ..
            }) => (doc, span.start),
            _ => continue,
        };
        let start_byte = cursor.byte_at(start_char as usize, source);
        *doc_slot = extract_doc_comment(source, &line_starts, start_byte);
    }
}

/// Forward-only converter from character offset to byte offset. Requires the
/// requested offsets to be non-decreasing; if an offset ever goes backwards it
/// falls back to a full rescan so correctness never depends on the ordering.
struct CharByteCursor<'a> {
    iter: std::str::CharIndices<'a>,
    /// Index of the next char the iterator will yield.
    next_index: usize,
    /// Byte offset of the most recently yielded char.
    cur_byte: usize,
}

impl<'a> CharByteCursor<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            iter: source.char_indices(),
            next_index: 0,
            cur_byte: 0,
        }
    }

    fn byte_at(&mut self, char_offset: usize, source: &str) -> usize {
        if char_offset == 0 {
            return 0;
        }
        // Offset went backwards (should not happen for top-level items): rescan.
        if char_offset + 1 < self.next_index {
            return char_offset_to_byte_offset(source, char_offset);
        }
        while self.next_index <= char_offset {
            match self.iter.next() {
                Some((byte_idx, _)) => {
                    self.cur_byte = byte_idx;
                    self.next_index += 1;
                }
                None => return source.len(),
            }
        }
        self.cur_byte
    }
}

fn extract_doc_comment(
    source: &str,
    line_starts: &[usize],
    start_byte_offset: usize,
) -> Option<String> {
    if line_starts.is_empty() {
        return None;
    }

    let decl_line_idx = match line_starts.binary_search(&start_byte_offset) {
        Ok(idx) => idx,
        Err(idx) => idx.saturating_sub(1),
    };

    if decl_line_idx == 0 {
        return None;
    }

    let mut line_idx = decl_line_idx as isize - 1;
    let line = line_text(source, line_starts, line_idx as usize);
    if line.trim().is_empty() {
        // A blank line directly above the declaration breaks doc attachment.
        return None;
    }

    let mut lines_rev = Vec::new();
    while line_idx >= 0 {
        let line = line_text(source, line_starts, line_idx as usize);
        let trimmed = line.trim_start();
        let Some(after_marker) = trimmed.strip_prefix("///") else {
            break;
        };
        let text = after_marker
            .strip_prefix(' ')
            .unwrap_or(after_marker)
            .trim_end_matches('\r')
            .to_string();
        lines_rev.push(text);
        line_idx -= 1;
    }

    if lines_rev.is_empty() {
        return None;
    }

    lines_rev.reverse();
    Some(lines_rev.join("\n"))
}

fn compute_line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (idx, ch) in source.char_indices() {
        if ch == '\n' {
            starts.push(idx + 1);
        }
    }
    starts
}

fn line_text<'a>(source: &'a str, line_starts: &[usize], line_idx: usize) -> &'a str {
    let start = line_starts.get(line_idx).copied().unwrap_or(0);
    let end = line_starts
        .get(line_idx + 1)
        .copied()
        .unwrap_or(source.len());
    source[start..end].trim_end_matches('\n')
}

fn char_offset_to_byte_offset(source: &str, char_offset: usize) -> usize {
    if char_offset == 0 {
        return 0;
    }
    for (seen, (byte_idx, _)) in source.char_indices().enumerate() {
        if seen == char_offset {
            return byte_idx;
        }
    }
    source.len()
}

fn format_lexer_error(registry: &FileRegistry, error: lexer::LexError) -> anyhow::Error {
    let file_name = registry.file_name(error.span.file_id).unwrap_or("unknown");

    if let Some((line, col)) = registry.line_col(error.span) {
        let mut msg = format!(
            "{}:{}:{}: Lexer error: {:?}",
            file_name, line, col, error.kind
        );

        // Add source context
        if let Some(line_text) = registry.line_text(error.span) {
            msg.push_str(&format!("\n{:4} | {}", line, line_text));

            // Add caret pointing to error
            let caret_pos = col + 6; // "  N | " prefix
            msg.push_str(&format!("\n{:>width$}", "^", width = caret_pos));
        }

        anyhow::anyhow!(msg)
    } else {
        anyhow::anyhow!("Lexer error at {:?}: {:?}", error.span, error.kind)
    }
}

fn format_parse_error(registry: &FileRegistry, error: parser::ParseError) -> anyhow::Error {
    let file_name = registry.file_name(error.span.file_id).unwrap_or("unknown");

    if let Some((line, col)) = registry.line_col(error.span) {
        let mut msg = match &error.kind {
            parser::ParseErrorKind::UnexpectedToken { expected, found } => {
                if expected.len() == 1 {
                    format!(
                        "{}:{}:{}: Expected {}, found {}",
                        file_name, line, col, expected[0], found
                    )
                } else {
                    format!(
                        "{}:{}:{}: Expected one of [{}], found {}",
                        file_name,
                        line,
                        col,
                        expected.join(", "),
                        found
                    )
                }
            }
            parser::ParseErrorKind::UnexpectedEof { expected } => {
                if expected.len() == 1 {
                    format!(
                        "{}:{}:{}: Unexpected end of file, expected {}",
                        file_name, line, col, expected[0]
                    )
                } else {
                    format!(
                        "{}:{}:{}: Unexpected end of file, expected one of [{}]",
                        file_name,
                        line,
                        col,
                        expected.join(", ")
                    )
                }
            }
            parser::ParseErrorKind::ConstructorInPostfix { name } => {
                format!(
                    "{}:{}:{}: Constructor '.{}' cannot appear after an expression",
                    file_name, line, col, name
                )
            }
            parser::ParseErrorKind::LowercaseVariant { name } => {
                format!(
                    "{}:{}:{}: Variant name '{}' must start with an uppercase letter",
                    file_name, line, col, name
                )
            }
            parser::ParseErrorKind::CaseViolation {
                kind,
                name,
                expected,
            } => {
                format!(
                    "{}:{}:{}: {} '{}' must start with {} letter",
                    file_name, line, col, kind, name, expected
                )
            }
            parser::ParseErrorKind::EmptyImportList => {
                format!(
                    "{}:{}:{}: Import list cannot be empty",
                    file_name, line, col
                )
            }
            parser::ParseErrorKind::StatementInExpression { statement, context } => {
                let ctx = match context {
                    Some(c) => format!(" in {c}"),
                    None => String::new(),
                };
                format!(
                    "{}:{}:{}: '{}' is a statement and cannot be used where an expression is expected{}\nhint: wrap it in a block expression, e.g. `=> {{ {} ... }}`",
                    file_name, line, col, statement, ctx, statement
                )
            }
        };

        // Add source context
        if let Some(line_text) = registry.line_text(error.span) {
            msg.push_str(&format!("\n{:4} | {}", line, line_text));

            // Add caret/underline pointing to error
            let span_len = (error.span.end - error.span.start).max(1) as usize;
            let caret_pos = col + 6; // "  N | " prefix
            let underline = if span_len == 1 {
                "^".to_string()
            } else {
                "^".to_string() + &"-".repeat(span_len.saturating_sub(1))
            };
            msg.push_str(&format!(
                "\n{:>width$}",
                underline,
                width = caret_pos + span_len - 1
            ));
        }

        anyhow::anyhow!(msg)
    } else {
        anyhow::anyhow!("Parse error at {:?}: {:?}", error.span, error.kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_comments_attach_to_next_function_decl() {
        let source = r#"/// Adds one.
/// Useful for tests.
fn add_one(x: Int) Int {
  x + 1
}
"#;

        let (ast, _) = parse_source(source, "test.tw").expect("parse should succeed");
        let Some(ast::Item::Function(decl)) = ast.items.first() else {
            panic!("expected first item to be a function");
        };
        assert_eq!(
            decl.doc.as_deref(),
            Some("Adds one.\nUseful for tests."),
            "expected contiguous /// lines to attach to function"
        );
    }

    #[test]
    fn blank_line_breaks_doc_comment_attachment() {
        let source = r#"/// Detached.

fn f() Int {
  1
}
"#;

        let (ast, _) = parse_source(source, "test.tw").expect("parse should succeed");
        let Some(ast::Item::Function(decl)) = ast.items.first() else {
            panic!("expected first item to be a function");
        };
        assert_eq!(
            decl.doc, None,
            "blank line between /// block and declaration should break attachment"
        );
    }

    #[test]
    fn non_doc_comment_does_not_attach_as_doc() {
        let source = r#"/// Doc line.
// regular note
fn f() Int {
  1
}
"#;

        let (ast, _) = parse_source(source, "test.tw").expect("parse should succeed");
        let Some(ast::Item::Function(decl)) = ast.items.first() else {
            panic!("expected first item to be a function");
        };
        assert_eq!(
            decl.doc, None,
            "non-doc comments are ignored and break contiguous doc attachment"
        );
    }

    #[test]
    fn doc_comments_attach_to_pub_let() {
        let source = r#"/// Exported answer.
pub answer := 42
"#;

        let (ast, _) = parse_source(source, "test.tw").expect("parse should succeed");
        let Some(ast::Item::Stmt(ast::Stmt::Let { doc, is_pub, .. })) = ast.items.first() else {
            panic!("expected first item to be a let statement");
        };
        assert!(*is_pub, "expected pub let");
        assert_eq!(doc.as_deref(), Some("Exported answer."));
    }

    #[test]
    fn parse_error_reports_statement_in_expression_with_hint() {
        let source = r#"fn f() Int {
  case 1 {
    1 => defer,
    _ => 0,
  }
}
"#;

        let err = parse_source(source, "test.tw").expect_err("parse should fail");
        let rendered = err.to_string();
        assert!(
            rendered.contains(
                "'defer' is a statement and cannot be used where an expression is expected"
            ),
            "expected clearer statement-vs-expression message, got: {rendered}"
        );
        assert!(
            rendered.contains("hint: wrap it in a block expression"),
            "expected fix hint in parse error, got: {rendered}"
        );
    }

    #[test]
    fn statement_in_expression_context_case_arm() {
        let source = "fn f() Int { case 1 { 1 => defer, _ => 0 } }";
        let err = parse_source(source, "test.tw").expect_err("parse should fail");
        let rendered = err.to_string();
        assert!(
            rendered.contains("case arm body"),
            "expected case-arm context, got: {rendered}"
        );
    }

    #[test]
    fn statement_in_expression_context_call_arg() {
        let source = "fn f() { foo(return 1) }";
        let err = parse_source(source, "test.tw").expect_err("parse should fail");
        let rendered = err.to_string();
        assert!(
            rendered.contains("call argument"),
            "expected call-argument context, got: {rendered}"
        );
    }

    #[test]
    fn statement_in_expression_context_array_element() {
        let source = "fn f() { [return 1] }";
        let err = parse_source(source, "test.tw").expect_err("parse should fail");
        let rendered = err.to_string();
        assert!(
            rendered.contains("array element"),
            "expected array-element context, got: {rendered}"
        );
    }

    #[test]
    fn statement_in_expression_context_grouped() {
        let source = "fn f() { (return 1) }";
        let err = parse_source(source, "test.tw").expect_err("parse should fail");
        let rendered = err.to_string();
        assert!(
            rendered.contains("grouped expression"),
            "expected grouped-expression context, got: {rendered}"
        );
    }
}
