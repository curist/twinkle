use crate::syntax::span::{FileRegistry, Span};
use std::fmt;

/// Errors that can occur during AST to Core IR lowering
#[derive(Debug, Clone)]
pub enum LowerError {
    /// Feature is not supported in this stage
    UnsupportedFeature { feature: &'static str, span: Span },

    /// Variant literal requires type context for resolution
    VariantNeedsTypeContext { span: Span },

    /// Record literal requires type context for resolution
    RecordNeedsTypeContext { span: Span },

    /// Unknown variant name for a sum type
    UnknownVariant {
        name: String,
        type_name: String,
        span: Span,
    },

    /// Unknown field name for a record type
    UnknownField {
        field: String,
        type_name: String,
        span: Span,
    },

    /// Pattern type mismatch
    PatternMismatch {
        expected: String,
        found: String,
        span: Span,
    },

    /// Result type not found (required for try desugaring)
    MissingResultType { span: Span },

    /// Internal error (should never happen if type checker is correct)
    InternalError { message: String, span: Span },
}

impl LowerError {
    /// Format error message with file context
    pub fn format(&self, registry: &FileRegistry) -> String {
        match self {
            LowerError::UnsupportedFeature { feature, span } => self.format_error(
                registry,
                *span,
                &format!("Unsupported feature: {}", feature),
                None,
            ),
            LowerError::VariantNeedsTypeContext { span } => self.format_error(
                registry,
                *span,
                "Variant literal requires type context",
                Some("Annotate the binding or use qualified name"),
            ),
            LowerError::RecordNeedsTypeContext { span } => self.format_error(
                registry,
                *span,
                "Record literal requires type context",
                Some("Annotate the binding or use qualified name"),
            ),
            LowerError::UnknownVariant {
                name,
                type_name,
                span,
            } => self.format_error(
                registry,
                *span,
                "Unknown variant",
                Some(&format!("Type '{}' has no variant '{}'", type_name, name)),
            ),
            LowerError::UnknownField {
                field,
                type_name,
                span,
            } => self.format_error(
                registry,
                *span,
                "Unknown field",
                Some(&format!("Type '{}' has no field '{}'", type_name, field)),
            ),
            LowerError::PatternMismatch {
                expected,
                found,
                span,
            } => self.format_error(
                registry,
                *span,
                "Pattern type mismatch",
                Some(&format!("Expected: {}, found: {}", expected, found)),
            ),
            LowerError::MissingResultType { span } => self.format_error(
                registry,
                *span,
                "'try' expression requires 'Result' type to be defined",
                Some("Define: type Result<T, E> = { Ok(T), Err(E) }"),
            ),
            LowerError::InternalError { message, span } => self.format_error(
                registry,
                *span,
                &format!("Internal lowering error: {}", message),
                Some("This is a compiler bug. Please report it."),
            ),
        }
    }

    fn format_error(
        &self,
        registry: &FileRegistry,
        span: Span,
        message: &str,
        note: Option<&str>,
    ) -> String {
        let file_name = registry.file_name(span.file_id).unwrap_or("unknown");

        if let Some((line, col)) = registry.line_col(span) {
            let mut output = format!("{}:{}:{}: error: {}", file_name, line, col, message);

            // Add source context
            if let Some(line_text) = registry.line_text(span) {
                output.push_str(&format!("\n{:4} | {}", line, line_text));

                // Add underline/caret
                let span_len = (span.end - span.start).max(1) as usize;
                let caret_pos = col + 6; // "  N | " prefix
                let underline = if span_len == 1 {
                    "^".to_string()
                } else {
                    "^".to_string() + &"-".repeat(span_len.saturating_sub(1))
                };
                output.push_str(&format!(
                    "\n{:>width$}",
                    underline,
                    width = caret_pos + span_len - 1
                ));
            }

            // Add note if provided
            if let Some(note_text) = note {
                output.push_str(&format!("\nnote: {}", note_text));
            }

            output
        } else {
            format!("error: {} (at {:?})", message, span)
        }
    }
}

impl fmt::Display for LowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LowerError::UnsupportedFeature { feature, .. } => {
                write!(f, "Unsupported feature: {}", feature)
            }
            LowerError::VariantNeedsTypeContext { .. } => {
                write!(f, "Variant literal requires type context")
            }
            LowerError::RecordNeedsTypeContext { .. } => {
                write!(f, "Record literal requires type context")
            }
            LowerError::UnknownVariant {
                name, type_name, ..
            } => {
                write!(f, "Unknown variant '{}' for type '{}'", name, type_name)
            }
            LowerError::UnknownField {
                field, type_name, ..
            } => {
                write!(f, "Unknown field '{}' for type '{}'", field, type_name)
            }
            LowerError::PatternMismatch {
                expected, found, ..
            } => {
                write!(
                    f,
                    "Pattern type mismatch: expected {}, found {}",
                    expected, found
                )
            }
            LowerError::MissingResultType { .. } => {
                write!(f, "'try' expression requires 'Result' type to be defined")
            }
            LowerError::InternalError { message, .. } => {
                write!(f, "Internal lowering error: {}", message)
            }
        }
    }
}

impl std::error::Error for LowerError {}
