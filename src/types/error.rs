use super::ty::MonoType;
use crate::syntax::span::{FileRegistry, Span};

/// Type error with source location
#[derive(Debug, Clone)]
pub enum TypeError {
    /// Undefined type name
    UndefinedType { name: String, span: Span },

    /// Undefined variable or function
    UndefinedVariable { name: String, span: Span },

    /// Type mismatch
    TypeMismatch {
        expected: MonoType,
        actual: MonoType,
        span: Span,
        /// Optional extra context, e.g. "argument 2 of call to `foo`"
        note: Option<String>,
    },

    /// Non-exhaustive pattern match
    NonExhaustiveMatch { missing: Vec<String>, span: Span },

    /// Tried to call a non-function value
    NotAFunction { ty: MonoType, span: Span },

    /// Wrong number of arguments in function call
    WrongArity {
        expected: usize,
        actual: usize,
        span: Span,
    },

    /// Field doesn't exist on record type
    NoSuchField {
        record_type: String,
        field: String,
        span: Span,
    },

    /// Variant doesn't exist on sum type
    NoSuchVariant {
        sum_type: String,
        variant: String,
        span: Span,
    },

    /// Duplicate type or function definition
    DuplicateDefinition {
        name: String,
        first: Span,
        second: Span,
    },

    /// Circular type alias
    CircularTypeAlias { name: String, span: Span },

    /// Anonymous record literal without expected type context
    AnonymousRecordWithoutContext { span: Span },

    /// Generic type parameters not supported in Stage 2
    GenericNotSupported {
        name: String,
        span: Span,
        note: String,
    },

    /// Feature not yet supported
    UnsupportedFeature {
        feature: &'static str,
        span: Span,
        note: String,
    },

    /// Invalid top-level item
    InvalidTopLevelItem { span: Span, note: String },

    /// Scrutinee of case expression must be a sum type
    CaseScrutineeNotSumType { actual_type: MonoType, span: Span },

    /// Field and method with the same name on a type
    FieldMethodCollision {
        type_name: String,
        name: String,
        span: Span,
    },

    /// Dict key type must be Int or String
    InvalidDictKey { key_type: MonoType, span: Span },

    /// Rebinding is not allowed at module scope
    ModuleScopeRebinding { name: String, span: Span },

    /// Occurs check failed: infinite type would be created
    OccursCheckFailed { span: Span },

    /// Binding type is ambiguous (contains unsolved MetaVars after checking)
    AmbiguousType {
        name: String,
        span: Span,
        note: String,
    },
}

impl TypeError {
    /// Format the error with source context using FileRegistry
    ///
    /// **Important**: Pass Some(type_env) to show readable type names like "Point".
    /// If you pass None, types will show as "Type#<id>" which is hard to read.
    ///
    /// All call sites (resolver, checker, CLI, tests) should thread the TypeEnv through:
    /// ```ignore
    /// error.format(&file_registry, Some(&type_env))
    /// ```
    pub fn format(
        &self,
        registry: &FileRegistry,
        type_env: Option<&super::env::TypeEnv>,
    ) -> String {
        // Helper to format a type with or without names
        let fmt_type = |ty: &MonoType| -> String {
            if let Some(env) = type_env {
                ty.format_with_names(env)
            } else {
                ty.to_string()
            }
        };

        match self {
            TypeError::UndefinedType { name, span } => {
                self.format_error(
                    registry,
                    *span,
                    &format!("Undefined type: {}", name),
                    None,
                )
            }
            TypeError::UndefinedVariable { name, span } => {
                self.format_error(
                    registry,
                    *span,
                    &format!("Undefined variable: {}", name),
                    None,
                )
            }
            TypeError::TypeMismatch {
                expected,
                actual,
                span,
                note,
            } => {
                let mut full_note = format!("Expected: {}\nActual:   {}", fmt_type(expected), fmt_type(actual));
                if let Some(ctx) = note {
                    full_note.push_str(&format!("\n{}", ctx));
                }
                self.format_error(registry, *span, "Type mismatch", Some(&full_note))
            }
            TypeError::NonExhaustiveMatch { missing, span } => self.format_error(
                registry,
                *span,
                &format!("Non-exhaustive pattern match"),
                Some(&format!(
                    "Missing patterns: {}",
                    missing.join(", ")
                )),
            ),
            TypeError::NotAFunction { ty, span } => self.format_error(
                registry,
                *span,
                &format!("Cannot call non-function value"),
                Some(&format!("Type: {}", fmt_type(ty))),
            ),
            TypeError::WrongArity {
                expected,
                actual,
                span,
            } => self.format_error(
                registry,
                *span,
                &format!("Wrong number of arguments"),
                Some(&format!(
                    "Expected {} argument(s), found {}",
                    expected, actual
                )),
            ),
            TypeError::NoSuchField {
                record_type,
                field,
                span,
            } => self.format_error(
                registry,
                *span,
                &format!("No such field"),
                Some(&format!(
                    "Type '{}' has no field '{}'",
                    record_type, field
                )),
            ),
            TypeError::NoSuchVariant {
                sum_type,
                variant,
                span,
            } => self.format_error(
                registry,
                *span,
                &format!("No such variant"),
                Some(&format!(
                    "Type '{}' has no variant '{}'",
                    sum_type, variant
                )),
            ),
            TypeError::DuplicateDefinition { name, first, second } => {
                let first_msg = self.format_error(
                    registry,
                    *first,
                    &format!("'{}' defined here first", name),
                    None,
                );
                let second_msg = self.format_error(
                    registry,
                    *second,
                    &format!("'{}' redefined here", name),
                    None,
                );
                format!("{}\n{}", first_msg, second_msg)
            }
            TypeError::CircularTypeAlias { name, span } => self.format_error(
                registry,
                *span,
                &format!("Circular type alias: {}", name),
                None,
            ),
            TypeError::AnonymousRecordWithoutContext { span } => self.format_error(
                registry,
                *span,
                &format!("Anonymous record literal requires type annotation"),
                Some("Try adding a type annotation or using a named constructor (Point.{{ ... }})"),
            ),
            TypeError::GenericNotSupported { name, span, note } => self.format_error(
                registry,
                *span,
                &format!("Generic type parameters not yet supported: {}", name),
                Some(note),
            ),
            TypeError::UnsupportedFeature { feature, span, note } => self.format_error(
                registry,
                *span,
                &format!("Unsupported feature: {}", feature),
                Some(note),
            ),
            TypeError::InvalidTopLevelItem { span, note } => self.format_error(
                registry,
                *span,
                &format!("Invalid top-level item"),
                Some(note),
            ),
            TypeError::CaseScrutineeNotSumType { actual_type, span } => self.format_error(
                registry,
                *span,
                &format!("Case scrutinee must be a sum type"),
                Some(&format!("Found type: {}", fmt_type(actual_type))),
            ),
            TypeError::FieldMethodCollision { type_name, name, span } => self.format_error(
                registry,
                *span,
                &format!("Field and method collision on type '{}'", type_name),
                Some(&format!(
                    "Both a field and a method named '{}' exist on this type. This is not allowed.",
                    name
                )),
            ),
            TypeError::ModuleScopeRebinding { name, span } => self.format_error(
                registry,
                *span,
                &format!("Cannot rebind '{}' at module scope", name),
                Some("Rebinding (=) is not allowed at module scope — each name may only be bound once.\nUse a new binding (`:=`) instead."),
            ),
            TypeError::InvalidDictKey { key_type, span } => self.format_error(
                registry,
                *span,
                "Invalid Dict key type",
                Some(&format!(
                    "Dict key must be Int or String, but got: {}\nBool, Float, and compound types are not allowed as Dict keys.",
                    fmt_type(key_type)
                )),
            ),
            TypeError::OccursCheckFailed { span } => self.format_error(
                registry,
                *span,
                "Occurs check failed: infinite type",
                Some("Cannot construct an infinite type"),
            ),
            TypeError::AmbiguousType { name, span, note } => self.format_error(
                registry,
                *span,
                &format!("Ambiguous type for '{}'", name),
                Some(note),
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
