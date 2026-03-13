use crate::query::api::{QueryDiagnostic, QuerySpan};
use crate::syntax::span::FileRegistry;

use super::position::byte_offset_to_position_utf16;

/// An LSP-ready diagnostic with UTF-16 range and severity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspDiagnostic {
    pub range: LspRange,
    pub severity: LspSeverity,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LspRange {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspSeverity {
    Error,
    Warning,
}

impl LspSeverity {
    /// LSP numeric severity value (1 = Error, 2 = Warning).
    pub fn to_lsp_number(self) -> u32 {
        match self {
            LspSeverity::Error => 1,
            LspSeverity::Warning => 2,
        }
    }
}

/// Convert a `QueryDiagnostic` to an `LspDiagnostic` using the file registry
/// for UTF-16 position mapping.
pub fn query_diagnostic_to_lsp(
    diag: &QueryDiagnostic,
    registry: &FileRegistry,
) -> Option<LspDiagnostic> {
    let range = match &diag.span {
        Some(span) => query_span_to_lsp_range(span, registry)?,
        None => LspRange {
            start_line: 0,
            start_character: 0,
            end_line: 0,
            end_character: 0,
        },
    };

    let severity = code_to_severity(diag.code);

    Some(LspDiagnostic {
        range,
        severity,
        code: diag.code.to_string(),
        message: diag.message.clone(),
    })
}

fn query_span_to_lsp_range(span: &QuerySpan, registry: &FileRegistry) -> Option<LspRange> {
    let file_id = crate::syntax::span::FileId(span.file_id);
    let source = registry.source(file_id)?;
    let start = byte_offset_to_position_utf16(source, span.start as usize)?;
    let end = byte_offset_to_position_utf16(source, span.end as usize)?;
    Some(LspRange {
        start_line: start.line,
        start_character: start.character,
        end_line: end.line,
        end_character: end.character,
    })
}

fn code_to_severity(code: &str) -> LspSeverity {
    if code.starts_with("W_") {
        LspSeverity::Warning
    } else {
        LspSeverity::Error
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::span::FileRegistry;

    fn make_registry(source: &str) -> (FileRegistry, u32) {
        let mut registry = FileRegistry::new();
        let file_id = registry.add_file("test.tw".to_string(), source.to_string());
        (registry, file_id.0)
    }

    #[test]
    fn converts_error_diagnostic_with_span() {
        let source = "abc\ndef\nghi";
        let (registry, file_id) = make_registry(source);

        let diag = QueryDiagnostic {
            code: "E_UNDEFINED_VARIABLE",
            message: "undefined variable 'x'".to_string(),
            span: Some(QuerySpan {
                file_id,
                line: 1,
                column: 0,
                start: 4,
                end: 7,
            }),
        };

        let lsp = query_diagnostic_to_lsp(&diag, &registry).expect("should convert");
        assert_eq!(lsp.severity, LspSeverity::Error);
        assert_eq!(lsp.code, "E_UNDEFINED_VARIABLE");
        assert_eq!(lsp.message, "undefined variable 'x'");
        assert_eq!(lsp.range.start_line, 1);
        assert_eq!(lsp.range.start_character, 0);
        assert_eq!(lsp.range.end_line, 1);
        assert_eq!(lsp.range.end_character, 3);
    }

    #[test]
    fn converts_warning_code_to_warning_severity() {
        let source = "abc";
        let (registry, file_id) = make_registry(source);

        let diag = QueryDiagnostic {
            code: "W_UNUSED_VARIABLE",
            message: "unused variable 'x'".to_string(),
            span: Some(QuerySpan {
                file_id,
                line: 0,
                column: 0,
                start: 0,
                end: 3,
            }),
        };

        let lsp = query_diagnostic_to_lsp(&diag, &registry).expect("should convert");
        assert_eq!(lsp.severity, LspSeverity::Warning);
    }

    #[test]
    fn spanless_diagnostic_uses_zero_range() {
        let (registry, _) = make_registry("abc");

        let diag = QueryDiagnostic {
            code: "E_CIRCULAR_IMPORT",
            message: "circular import detected".to_string(),
            span: None,
        };

        let lsp = query_diagnostic_to_lsp(&diag, &registry).expect("should convert");
        assert_eq!(lsp.range.start_line, 0);
        assert_eq!(lsp.range.start_character, 0);
        assert_eq!(lsp.range.end_line, 0);
        assert_eq!(lsp.range.end_character, 0);
    }

    #[test]
    fn multibyte_span_uses_utf16_offsets() {
        // "a😀b\n" — 😀 is 4 UTF-8 bytes, 2 UTF-16 code units
        let source = "a😀b\nxyz";
        let (registry, file_id) = make_registry(source);

        // span covers "b" which starts at byte 5 (a=1, 😀=4), ends at byte 6
        let diag = QueryDiagnostic {
            code: "E_TYPE_MISMATCH",
            message: "type mismatch".to_string(),
            span: Some(QuerySpan {
                file_id,
                line: 0,
                column: 5,
                start: 5,
                end: 6,
            }),
        };

        let lsp = query_diagnostic_to_lsp(&diag, &registry).expect("should convert");
        // 'a' = 1 UTF-16 unit, '😀' = 2 UTF-16 units, so 'b' starts at character 3
        assert_eq!(lsp.range.start_line, 0);
        assert_eq!(lsp.range.start_character, 3);
        assert_eq!(lsp.range.end_line, 0);
        assert_eq!(lsp.range.end_character, 4);
    }

    #[test]
    fn severity_lsp_numbers() {
        assert_eq!(LspSeverity::Error.to_lsp_number(), 1);
        assert_eq!(LspSeverity::Warning.to_lsp_number(), 2);
    }
}
