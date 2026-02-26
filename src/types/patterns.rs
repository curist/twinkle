use crate::syntax::ast::{CaseArm, Literal, Pattern};
use crate::syntax::span::Span;
use super::env::{LocalEnv, TypeEnv};
use super::error::TypeError;
use super::ty::{MonoType, OPTION_TYPE_ID, RESULT_TYPE_ID};
use std::collections::HashSet;

/// Pattern checking utilities for case expressions
pub struct PatternChecker<'a> {
    type_env: &'a TypeEnv,
    local_env: &'a mut LocalEnv,
    errors: &'a mut Vec<TypeError>,
}

impl<'a> PatternChecker<'a> {
    pub fn new(
        type_env: &'a TypeEnv,
        local_env: &'a mut LocalEnv,
        errors: &'a mut Vec<TypeError>,
    ) -> Self {
        Self {
            type_env,
            local_env,
            errors,
        }
    }

    /// Check a pattern against an expected type and bind variables
    pub fn check_pattern(&mut self, pattern: &Pattern, expected: &MonoType) -> Result<(), ()> {
        match pattern {
            Pattern::Wildcard(_) => {
                // Wildcard matches anything, no bindings
                Ok(())
            }

            Pattern::Ident(name, _) => {
                // Identifier pattern binds the entire value
                self.local_env.bind(name.clone(), expected.clone());
                Ok(())
            }

            Pattern::Literal(lit, span) => {
                // Literal pattern must match the expected type
                let lit_ty = match lit {
                    Literal::Int(_) => MonoType::Int,
                    Literal::Float(_) => MonoType::Float,
                    Literal::Bool(_) => MonoType::Bool,
                    Literal::String(_) => MonoType::String,
                };

                if &lit_ty == expected {
                    Ok(())
                } else {
                    self.errors.push(TypeError::TypeMismatch {
                        expected: expected.clone(),
                        actual: lit_ty,
                        span: *span,
                    note: None,
                    });
                    Err(())
                }
            }

            Pattern::Variant {
                name,
                fields,
                span,
            } => {
                // Variant pattern must match a sum type
                match expected {
                    MonoType::Named { type_id, args } => {
                        // Get the variant definition
                        let variants = match self.type_env.get_variants(*type_id) {
                            Some(v) => v,
                            None => {
                                // Not a sum type
                                self.errors.push(TypeError::TypeMismatch {
                                    expected: expected.clone(),
                                    actual: MonoType::Void, // Dummy
                                    span: *span,
                    note: None,
                                });
                                return Err(());
                            }
                        };

                        // Find the matching variant
                        let variant = variants.iter().find(|v| &v.name == name);

                        match variant {
                            Some(v) => {
                                // For Option<T> and Result<T,E>, the TypeDef holds placeholder
                                // Void fields. Use the actual type args from the MonoType.
                                let actual_field_tys: Vec<MonoType> =
                                    if *type_id == OPTION_TYPE_ID {
                                        match name.as_str() {
                                            "None" => vec![],
                                            "Some" => vec![args.first().cloned().unwrap_or(MonoType::Void)],
                                            _ => v.fields.clone(),
                                        }
                                    } else if *type_id == RESULT_TYPE_ID {
                                        match name.as_str() {
                                            "Ok"  => vec![args.first().cloned().unwrap_or(MonoType::Void)],
                                            "Err" => vec![args.get(1).cloned().unwrap_or(MonoType::Void)],
                                            _ => v.fields.clone(),
                                        }
                                    } else {
                                        v.fields.clone()
                                    };

                                // Check arity
                                if actual_field_tys.len() != fields.len() {
                                    self.errors.push(TypeError::WrongArity {
                                        expected: actual_field_tys.len(),
                                        actual: fields.len(),
                                        span: *span,
                                    });
                                    return Err(());
                                }

                                // Check each field pattern
                                for (field_pattern, field_ty) in fields.iter().zip(actual_field_tys.iter())
                                {
                                    self.check_pattern(field_pattern, field_ty)?;
                                }

                                Ok(())
                            }
                            None => {
                                // Variant not found
                                let sum_type_name = self
                                    .type_env
                                    .get_def(*type_id)
                                    .map(|d| d.name().to_string())
                                    .unwrap_or_else(|| format!("Type#{}", type_id.0));

                                self.errors.push(TypeError::NoSuchVariant {
                                    sum_type: sum_type_name,
                                    variant: name.clone(),
                                    span: *span,
                                });
                                Err(())
                            }
                        }
                    }
                    _ => {
                        // Expected type is not a sum type
                        self.errors.push(TypeError::CaseScrutineeNotSumType {
                            actual_type: expected.clone(),
                            span: *span,
                        });
                        Err(())
                    }
                }
            }
        }
    }

    /// Check exhaustiveness of case patterns
    /// Returns Ok if patterns are exhaustive, Err with missing variants otherwise
    pub fn check_exhaustiveness(
        type_env: &TypeEnv,
        errors: &mut Vec<TypeError>,
        scrut_ty: &MonoType,
        arms: &[CaseArm],
        span: Span,
    ) -> Result<(), ()> {
        // Get the type_id for the sum type
        let type_id = match scrut_ty {
            MonoType::Named { type_id, .. } => type_id,
            _ => {
                // Scrutinee must be a sum type (should have been checked earlier)
                return Ok(());
            }
        };

        // Get all variants of the sum type
        let variants = match type_env.get_variants(*type_id) {
            Some(v) => v,
            None => {
                // Not a sum type (should have been checked earlier)
                return Ok(());
            }
        };

        // Collect all variant names that must be covered
        let mut required_variants: HashSet<String> =
            variants.iter().map(|v| v.name.clone()).collect();

        // Check if there's a wildcard pattern
        let mut has_wildcard = false;

        // Mark covered variants
        for arm in arms {
            match &arm.pattern {
                Pattern::Wildcard(_) | Pattern::Ident(_, _) => {
                    // Wildcard or identifier pattern covers all variants
                    has_wildcard = true;
                    break;
                }
                Pattern::Variant { name, .. } => {
                    required_variants.remove(name);
                }
                Pattern::Literal(_, _) => {
                    // Literal patterns don't cover variants
                    // This is actually an error case but will be caught by pattern checking
                }
            }
        }

        // If there's a wildcard, we're exhaustive
        if has_wildcard {
            return Ok(());
        }

        // If there are uncovered variants, report error
        if !required_variants.is_empty() {
            let missing: Vec<String> = required_variants.into_iter().collect();
            errors.push(TypeError::NonExhaustiveMatch { missing, span });
            return Err(());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::span::FileId;
    use crate::types::ty::{TypeDef, Variant};

    #[test]
    fn test_pattern_wildcard() {
        let type_env = TypeEnv::new();
        let mut local_env = LocalEnv::new();
        let mut errors = Vec::new();

        let mut checker = PatternChecker::new(&type_env, &mut local_env, &mut errors);

        let pattern = Pattern::Wildcard(Span::new(FileId(0), 0, 1));
        let result = checker.check_pattern(&pattern, &MonoType::Int);

        assert!(result.is_ok());
        assert!(errors.is_empty());
    }

    #[test]
    fn test_pattern_ident_binds() {
        let type_env = TypeEnv::new();
        let mut local_env = LocalEnv::new();
        let mut errors = Vec::new();

        let mut checker = PatternChecker::new(&type_env, &mut local_env, &mut errors);

        let pattern = Pattern::Ident("x".to_string(), Span::new(FileId(0), 0, 1));
        let result = checker.check_pattern(&pattern, &MonoType::Int);

        assert!(result.is_ok());
        assert!(errors.is_empty());
        assert_eq!(local_env.lookup("x"), Some(&MonoType::Int));
    }

    #[test]
    fn test_pattern_literal_match() {
        let type_env = TypeEnv::new();
        let mut local_env = LocalEnv::new();
        let mut errors = Vec::new();

        let mut checker = PatternChecker::new(&type_env, &mut local_env, &mut errors);

        let pattern = Pattern::Literal(Literal::Int(42), Span::new(FileId(0), 0, 2));
        let result = checker.check_pattern(&pattern, &MonoType::Int);

        assert!(result.is_ok());
        assert!(errors.is_empty());
    }

    #[test]
    fn test_pattern_literal_mismatch() {
        let type_env = TypeEnv::new();
        let mut local_env = LocalEnv::new();
        let mut errors = Vec::new();

        let mut checker = PatternChecker::new(&type_env, &mut local_env, &mut errors);

        let pattern = Pattern::Literal(Literal::Int(42), Span::new(FileId(0), 0, 2));
        let result = checker.check_pattern(&pattern, &MonoType::String);

        assert!(result.is_err());
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], TypeError::TypeMismatch { .. }));
    }

    #[test]
    fn test_exhaustiveness_with_wildcard() {
        let mut type_env = TypeEnv::new();
        let type_id = type_env.add_type(TypeDef::Sum {
            name: "Option".to_string(),
            variants: vec![
                Variant {
                    name: "None".to_string(),
                    fields: vec![],
                },
                Variant {
                    name: "Some".to_string(),
                    fields: vec![MonoType::Int],
                },
            ],
        });

        let scrut_ty = MonoType::named(type_id);
        let arms = vec![CaseArm {
            pattern: Pattern::Wildcard(Span::new(FileId(0), 0, 1)),
            body: crate::syntax::ast::Expr::new(
                crate::syntax::ast::ExprId(0),
                crate::syntax::ast::ExprKind::Literal(Literal::Int(0)),
                Span::new(FileId(0), 0, 1),
            ),
            span: Span::new(FileId(0), 0, 1),
        }];

        let mut errors = Vec::new();
        let result = PatternChecker::check_exhaustiveness(
            &type_env,
            &mut errors,
            &scrut_ty,
            &arms,
            Span::new(FileId(0), 0, 1),
        );

        assert!(result.is_ok());
        assert!(errors.is_empty());
    }

    #[test]
    fn test_exhaustiveness_missing_variant() {
        let mut type_env = TypeEnv::new();
        let type_id = type_env.add_type(TypeDef::Sum {
            name: "Option".to_string(),
            variants: vec![
                Variant {
                    name: "None".to_string(),
                    fields: vec![],
                },
                Variant {
                    name: "Some".to_string(),
                    fields: vec![MonoType::Int],
                },
            ],
        });

        let scrut_ty = MonoType::named(type_id);
        let arms = vec![CaseArm {
            pattern: Pattern::Variant {
                name: "None".to_string(),
                fields: vec![],
                span: Span::new(FileId(0), 0, 4),
            },
            body: crate::syntax::ast::Expr::new(
                crate::syntax::ast::ExprId(0),
                crate::syntax::ast::ExprKind::Literal(Literal::Int(0)),
                Span::new(FileId(0), 0, 1),
            ),
            span: Span::new(FileId(0), 0, 1),
        }];

        let mut errors = Vec::new();
        let result = PatternChecker::check_exhaustiveness(
            &type_env,
            &mut errors,
            &scrut_ty,
            &arms,
            Span::new(FileId(0), 0, 1),
        );

        assert!(result.is_err());
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], TypeError::NonExhaustiveMatch { .. }));
    }
}
