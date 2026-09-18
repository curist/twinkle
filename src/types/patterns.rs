use super::check::{apply_subst, build_type_subst};
use super::env::{LocalEnv, TypeEnv};
use super::error::TypeError;
use super::ty::{
    MonoType, OPTION_TYPE_ID, RESULT_TYPE_ID, TUPLE2_TYPE_ID, TUPLE3_TYPE_ID, TUPLE4_TYPE_ID,
};
use crate::syntax::ast::{CaseArm, Literal, Pattern};
use crate::syntax::span::Span;
use std::collections::HashSet;

/// Resolve the compiler-known TupleN TypeId for a given arity (2..=4).
fn tuple_type_id_for_arity(arity: usize) -> Option<super::ty::TypeId> {
    match arity {
        2 => Some(TUPLE2_TYPE_ID),
        3 => Some(TUPLE3_TYPE_ID),
        4 => Some(TUPLE4_TYPE_ID),
        _ => None,
    }
}

/// Is this MonoType one of the compiler-known TupleN records?
fn is_tuple_named(type_id: super::ty::TypeId) -> bool {
    matches!(type_id, TUPLE2_TYPE_ID | TUPLE3_TYPE_ID | TUPLE4_TYPE_ID)
}

/// A pattern is irrefutable if it always matches, regardless of scrutinee value.
/// Tuple patterns are irrefutable iff every element pattern is irrefutable.
pub fn pattern_is_irrefutable(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Wildcard(_) | Pattern::Ident(_, _) => true,
        Pattern::Tuple(subs, _) => subs.iter().all(pattern_is_irrefutable),
        Pattern::Literal(_, _) | Pattern::Variant { .. } => false,
    }
}

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
    #[allow(clippy::result_unit_err)]
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

                // Int literals are accepted for Byte scrutinees, with range check
                if let (Literal::Int(n), MonoType::Byte) = (lit, expected) {
                    if !(0..=255).contains(n) {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Byte,
                            actual: MonoType::Int,
                            span: *span,
                            note: Some(format!(
                                "integer literal {} is out of range for Byte (0..255)",
                                n
                            )),
                        });
                        return Err(());
                    }
                    return Ok(());
                }

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
                type_name: _,
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
                                let actual_field_tys: Vec<MonoType> = if *type_id == OPTION_TYPE_ID
                                {
                                    match name.as_str() {
                                        "None" => vec![],
                                        "Some" => {
                                            vec![args.first().cloned().unwrap_or(MonoType::Void)]
                                        }
                                        _ => v.fields.clone(),
                                    }
                                } else if *type_id == RESULT_TYPE_ID {
                                    match name.as_str() {
                                        "Ok" => {
                                            vec![args.first().cloned().unwrap_or(MonoType::Void)]
                                        }
                                        "Err" => {
                                            vec![args.get(1).cloned().unwrap_or(MonoType::Void)]
                                        }
                                        _ => v.fields.clone(),
                                    }
                                } else {
                                    // User-defined generic sum type: apply type-arg substitution
                                    let type_params = self
                                        .type_env
                                        .get_def(*type_id)
                                        .map(|d| d.type_params().to_vec())
                                        .unwrap_or_default();
                                    let subst = build_type_subst(&type_params, args);
                                    v.fields.iter().map(|f| apply_subst(f, &subst)).collect()
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
                                for (field_pattern, field_ty) in
                                    fields.iter().zip(actual_field_tys.iter())
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

            Pattern::Tuple(subs, span) => {
                // Resolve the TupleN TypeId by arity. The parser only ever
                // produces arity 2..=4 (same limit as tuple literals), but
                // guard defensively in case that invariant ever slips.
                let Some(pattern_type_id) = tuple_type_id_for_arity(subs.len()) else {
                    self.errors.push(TypeError::UnsupportedFeature {
                        feature: "tuple patterns",
                        span: *span,
                        note: format!(
                            "tuple patterns support 2 to 4 elements, got {}",
                            subs.len()
                        ),
                    });
                    return Err(());
                };

                match expected {
                    // Same arity as the scrutinee's tuple type: recurse per element.
                    MonoType::Named { type_id, args }
                        if *type_id == pattern_type_id && args.len() == subs.len() =>
                    {
                        for (sub, arg_ty) in subs.iter().zip(args.iter()) {
                            self.check_pattern(sub, arg_ty)?;
                        }
                        Ok(())
                    }
                    // Scrutinee is a tuple, but of a different arity.
                    MonoType::Named { type_id, args } if is_tuple_named(*type_id) => {
                        self.errors.push(TypeError::WrongArity {
                            expected: args.len(),
                            actual: subs.len(),
                            span: *span,
                        });
                        Err(())
                    }
                    // Scrutinee isn't a tuple type at all.
                    _ => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: expected.clone(),
                            actual: MonoType::Void, // Dummy — no concrete type inferred yet
                            span: *span,
                            note: Some(
                                "tuple pattern requires a tuple-typed scrutinee".to_string(),
                            ),
                        });
                        Err(())
                    }
                }
            }
        }
    }

    /// Check exhaustiveness of case patterns
    /// Returns Ok if patterns are exhaustive, Err with missing variants otherwise
    #[allow(clippy::result_unit_err)]
    pub fn check_exhaustiveness(
        type_env: &TypeEnv,
        errors: &mut Vec<TypeError>,
        scrut_ty: &MonoType,
        arms: &[CaseArm],
        span: Span,
    ) -> Result<(), ()> {
        // Bool has a finite literal domain, so `true` + `false` is exhaustive.
        // Other primitive domains remain open-ended and require a catch-all arm.
        if matches!(scrut_ty, MonoType::Bool) {
            let has_catch_all = arms
                .iter()
                .any(|arm| matches!(arm.pattern, Pattern::Wildcard(_) | Pattern::Ident(_, _)));
            if has_catch_all {
                return Ok(());
            }

            let mut has_true = false;
            let mut has_false = false;
            for arm in arms {
                if let Pattern::Literal(crate::syntax::ast::Literal::Bool(value), _) = &arm.pattern
                {
                    if *value {
                        has_true = true;
                    } else {
                        has_false = true;
                    }
                }
            }

            let mut missing = Vec::new();
            if !has_true {
                missing.push("true".to_string());
            }
            if !has_false {
                missing.push("false".to_string());
            }
            if !missing.is_empty() {
                errors.push(TypeError::NonExhaustiveMatch { missing, span });
                return Err(());
            }
            return Ok(());
        }

        // For open-ended primitive types, only a wildcard/identifier arm is exhaustive.
        if matches!(scrut_ty, MonoType::Int | MonoType::String | MonoType::Byte) {
            let has_wildcard = arms
                .iter()
                .any(|arm| matches!(arm.pattern, Pattern::Wildcard(_) | Pattern::Ident(_, _)));
            if !has_wildcard {
                errors.push(TypeError::NonExhaustiveMatch {
                    missing: vec!["_ (wildcard required for primitive match)".to_string()],
                    span,
                });
                return Err(());
            }
            return Ok(());
        }

        // Tuple scrutinee: conservative rule (v1). TupleN is a record, not a
        // sum type, so it has no variant set to track coverage against.
        // Instead: any arm whose pattern is fully irrefutable (a bare `_`/
        // identifier, or a tuple pattern whose every element is irrefutable)
        // covers all cases. A case with only refutable tuple arms and no
        // wildcard is reported non-exhaustive.
        if let MonoType::Named { type_id, .. } = scrut_ty
            && is_tuple_named(*type_id)
        {
            let is_exhaustive = arms.iter().any(|arm| pattern_is_irrefutable(&arm.pattern));
            if is_exhaustive {
                return Ok(());
            }
            errors.push(TypeError::NonExhaustiveMatch {
                missing: vec!["_ (wildcard required for tuple match)".to_string()],
                span,
            });
            return Err(());
        }

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
                Pattern::Tuple(_, _) => {
                    // Tuple patterns don't cover variants; check_pattern already
                    // rejects them against a sum-type scrutinee.
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
            type_params: vec![],
            variants: vec![
                Variant {
                    name: "None".to_string(),
                    fields: vec![],
                    tag: 0,
                },
                Variant {
                    name: "Some".to_string(),
                    fields: vec![MonoType::Int],
                    tag: 1,
                },
            ],
            doc: None,
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
            type_params: vec![],
            variants: vec![
                Variant {
                    name: "None".to_string(),
                    fields: vec![],
                    tag: 0,
                },
                Variant {
                    name: "Some".to_string(),
                    fields: vec![MonoType::Int],
                    tag: 1,
                },
            ],
            doc: None,
        });

        let scrut_ty = MonoType::named(type_id);
        let arms = vec![CaseArm {
            pattern: Pattern::Variant {
                type_name: None,
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

    #[test]
    fn test_pattern_tuple_binds_elements() {
        let type_env = TypeEnv::new();
        let mut local_env = LocalEnv::new();
        let mut errors = Vec::new();

        let mut checker = PatternChecker::new(&type_env, &mut local_env, &mut errors);

        let pattern = Pattern::Tuple(
            vec![
                Pattern::Ident("a".to_string(), Span::new(FileId(0), 0, 1)),
                Pattern::Ident("b".to_string(), Span::new(FileId(0), 1, 2)),
            ],
            Span::new(FileId(0), 0, 2),
        );
        let expected = MonoType::Named {
            type_id: TUPLE2_TYPE_ID,
            args: vec![MonoType::Int, MonoType::Int],
        };
        let result = checker.check_pattern(&pattern, &expected);

        assert!(result.is_ok());
        assert!(errors.is_empty());
        assert_eq!(local_env.lookup("a"), Some(&MonoType::Int));
        assert_eq!(local_env.lookup("b"), Some(&MonoType::Int));
    }

    #[test]
    fn test_pattern_tuple_element_type_flows_to_binding() {
        let type_env = TypeEnv::new();
        let mut local_env = LocalEnv::new();
        let mut errors = Vec::new();

        let mut checker = PatternChecker::new(&type_env, &mut local_env, &mut errors);

        let pattern = Pattern::Tuple(
            vec![
                Pattern::Ident("a".to_string(), Span::new(FileId(0), 0, 1)),
                Pattern::Ident("b".to_string(), Span::new(FileId(0), 1, 2)),
            ],
            Span::new(FileId(0), 0, 2),
        );
        let expected = MonoType::Named {
            type_id: TUPLE2_TYPE_ID,
            args: vec![MonoType::Int, MonoType::String],
        };
        let result = checker.check_pattern(&pattern, &expected);

        assert!(result.is_ok());
        assert!(errors.is_empty());
        // Each element must get its own distinct type, not a shared one.
        assert_eq!(local_env.lookup("a"), Some(&MonoType::Int));
        assert_eq!(local_env.lookup("b"), Some(&MonoType::String));
    }

    #[test]
    fn test_pattern_tuple_arity_mismatch() {
        let type_env = TypeEnv::new();
        let mut local_env = LocalEnv::new();
        let mut errors = Vec::new();

        let mut checker = PatternChecker::new(&type_env, &mut local_env, &mut errors);

        // Pattern has 3 elements, but the scrutinee is a Tuple2.
        let pattern = Pattern::Tuple(
            vec![
                Pattern::Ident("a".to_string(), Span::new(FileId(0), 0, 1)),
                Pattern::Ident("b".to_string(), Span::new(FileId(0), 1, 2)),
                Pattern::Ident("c".to_string(), Span::new(FileId(0), 2, 3)),
            ],
            Span::new(FileId(0), 0, 3),
        );
        let expected = MonoType::Named {
            type_id: TUPLE2_TYPE_ID,
            args: vec![MonoType::Int, MonoType::Int],
        };
        let result = checker.check_pattern(&pattern, &expected);

        assert!(result.is_err());
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            TypeError::WrongArity {
                expected: 2,
                actual: 3,
                ..
            }
        ));
    }

    #[test]
    fn test_pattern_tuple_against_non_tuple_type_mismatches() {
        let type_env = TypeEnv::new();
        let mut local_env = LocalEnv::new();
        let mut errors = Vec::new();

        let mut checker = PatternChecker::new(&type_env, &mut local_env, &mut errors);

        let pattern = Pattern::Tuple(
            vec![
                Pattern::Ident("a".to_string(), Span::new(FileId(0), 0, 1)),
                Pattern::Ident("b".to_string(), Span::new(FileId(0), 1, 2)),
            ],
            Span::new(FileId(0), 0, 2),
        );
        let result = checker.check_pattern(&pattern, &MonoType::Int);

        assert!(result.is_err());
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], TypeError::TypeMismatch { .. }));
    }

    #[test]
    fn test_exhaustiveness_tuple_irrefutable_arm_is_exhaustive() {
        let type_env = TypeEnv::new();
        let scrut_ty = MonoType::Named {
            type_id: TUPLE2_TYPE_ID,
            args: vec![MonoType::Int, MonoType::Int],
        };

        // (a, b) — fully irrefutable tuple arm covers every case.
        let arms = vec![CaseArm {
            pattern: Pattern::Tuple(
                vec![
                    Pattern::Ident("a".to_string(), Span::new(FileId(0), 0, 1)),
                    Pattern::Ident("b".to_string(), Span::new(FileId(0), 1, 2)),
                ],
                Span::new(FileId(0), 0, 2),
            ),
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
    fn test_exhaustiveness_tuple_refutable_only_is_non_exhaustive() {
        let type_env = TypeEnv::new();
        let scrut_ty = MonoType::Named {
            type_id: TUPLE2_TYPE_ID,
            args: vec![MonoType::Int, MonoType::Int],
        };

        // (1, b) — the literal element makes this arm refutable, and there's
        // no wildcard/irrefutable arm to cover the rest.
        let arms = vec![CaseArm {
            pattern: Pattern::Tuple(
                vec![
                    Pattern::Literal(Literal::Int(1), Span::new(FileId(0), 0, 1)),
                    Pattern::Ident("b".to_string(), Span::new(FileId(0), 1, 2)),
                ],
                Span::new(FileId(0), 0, 2),
            ),
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
