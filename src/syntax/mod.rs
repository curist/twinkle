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
    let source_file = parser
        .parse_source_file()
        .map_err(|e| format_parse_error(&registry, e))?;

    Ok((source_file, registry))
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
