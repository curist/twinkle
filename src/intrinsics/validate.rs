use anyhow::{Result, bail};

use crate::types::env::ValueEnv;
use crate::types::ty::{FunctionSignature, MonoType};

use super::{registry, signatures};

/// Check whether two MonoTypes are structurally equal, allowing type variable
/// renaming via the mapping from contract type param names to .tw stub names.
fn types_match(
    contract_ty: &MonoType,
    stub_ty: &MonoType,
    var_map: &std::collections::HashMap<&str, &str>,
) -> bool {
    match (contract_ty, stub_ty) {
        (MonoType::Var(a), MonoType::Var(b)) => var_map
            .get(a.as_str())
            .map_or(a == b, |mapped| *mapped == b.as_str()),
        (MonoType::Int, MonoType::Int)
        | (MonoType::Float, MonoType::Float)
        | (MonoType::Bool, MonoType::Bool)
        | (MonoType::String, MonoType::String)
        | (MonoType::Byte, MonoType::Byte)
        | (MonoType::Void, MonoType::Void) => true,
        (MonoType::Vector(a), MonoType::Vector(b)) => types_match(a, b, var_map),
        (MonoType::Dict(ak, av), MonoType::Dict(bk, bv)) => {
            types_match(ak, bk, var_map) && types_match(av, bv, var_map)
        }
        (
            MonoType::Named {
                type_id: a_id,
                args: a_args,
            },
            MonoType::Named {
                type_id: b_id,
                args: b_args,
            },
        ) => {
            a_id == b_id
                && a_args.len() == b_args.len()
                && a_args
                    .iter()
                    .zip(b_args)
                    .all(|(a, b)| types_match(a, b, var_map))
        }
        (
            MonoType::Function {
                params: ap,
                ret: ar,
            },
            MonoType::Function {
                params: bp,
                ret: br,
            },
        ) => {
            ap.len() == bp.len()
                && ap.iter().zip(bp).all(|(a, b)| types_match(a, b, var_map))
                && types_match(ar, br, var_map)
        }
        _ => false,
    }
}

fn compare_signature_shape(
    spec: &registry::IntrinsicSpec,
    sig: &FunctionSignature,
    expected: &signatures::IntrinsicContract,
    errors: &mut Vec<String>,
) {
    let label = || format!("FuncId({}) {}", spec.func_id.0, spec.twinkle_name);

    if sig.type_params.len() != expected.type_params.len() {
        errors.push(format!(
            "{}: generic count mismatch (stub={}, contract={})",
            label(),
            sig.type_params.len(),
            expected.type_params.len()
        ));
        return; // type param mismatch makes further comparison unreliable
    }

    // Build mapping from contract type param names to .tw stub type param names.
    let var_map: std::collections::HashMap<&str, &str> = expected
        .type_params
        .iter()
        .zip(sig.type_params.iter())
        .map(|(c, s)| (c.as_str(), s.as_str()))
        .collect();

    // IntrinsicContract has no bounds field — flag if the .tw stub declares
    // bounds on any type parameter, since they would be silently ignored.
    for (contract_param, stub_param) in expected.type_params.iter().zip(sig.type_params.iter()) {
        if let Some(bounds) = sig.type_param_bounds.get(stub_param.as_str())
            && !bounds.is_empty()
        {
            errors.push(format!(
                "{}: stub has bounds {:?} on type param '{}' but contract has no bounds field",
                label(),
                bounds,
                stub_param
            ));
        }
        let _ = contract_param; // used only for 1:1 zip alignment
    }

    if sig.params.len() != expected.params.len() {
        errors.push(format!(
            "{}: arity mismatch (stub={}, contract={})",
            label(),
            sig.params.len(),
            expected.params.len()
        ));
    } else {
        for (i, (stub_ty, contract_ty)) in sig.params.iter().zip(expected.params.iter()).enumerate()
        {
            if !types_match(contract_ty, stub_ty, &var_map) {
                errors.push(format!(
                    "{}: param {} type mismatch (stub={:?}, contract={:?})",
                    label(),
                    i,
                    stub_ty,
                    contract_ty
                ));
            }
        }
    }

    match &sig.ret {
        Some(stub_ret) => {
            if !types_match(&expected.ret, stub_ret, &var_map) {
                errors.push(format!(
                    "{}: return type mismatch (stub={:?}, contract={:?})",
                    label(),
                    stub_ret,
                    expected.ret
                ));
            }
        }
        None if expected.ret != MonoType::Void => {
            errors.push(format!(
                "{}: stub has no return type but contract expects {:?}",
                label(),
                expected.ret
            ));
        }
        None => {}
    }
}

/// Validate intrinsic bindings against the current ValueEnv signatures.
///
/// For every `IntrinsicSpec` with `include_in_signature_registry`:
/// - Must have a matching `IntrinsicContract` entry.
/// - Must be present in `ValueEnv` (populated from `.tw` signature stubs).
/// - Generic count, parameter count, parameter types, and return type
///   from the Rust-side `IntrinsicContract` must match the `.tw` stubs.
pub fn validate_intrinsic_bindings(value_env: &ValueEnv) -> Result<()> {
    let mut errors = Vec::new();

    for spec in registry::all_specs() {
        let expected_sig = if spec.include_in_signature_registry {
            signatures::contract(spec.func_id)
        } else {
            None
        };
        let has_expected_sig = expected_sig.is_some();

        if has_expected_sig != spec.include_in_signature_registry {
            errors.push(format!(
                "FuncId({}) {}: signature registry flag mismatch (spec={}, signature={})",
                spec.func_id.0,
                spec.twinkle_name,
                spec.include_in_signature_registry,
                has_expected_sig
            ));
        }

        let env_sig = value_env.get_function(spec.twinkle_name);

        if spec.include_in_signature_registry && env_sig.is_none() {
            errors.push(format!(
                "FuncId({}) {}: missing from ValueEnv signature registry",
                spec.func_id.0, spec.twinkle_name
            ));
            continue;
        }

        if let (Some(sig), Some(expected_sig)) = (env_sig, expected_sig.as_ref()) {
            compare_signature_shape(spec, sig, expected_sig, &mut errors);
        }
    }

    if errors.is_empty() {
        return Ok(());
    }

    bail!(
        "intrinsic signature validation failed:\n{}",
        errors.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_intrinsic_bindings_passes_for_default_env() {
        let env = ValueEnv::new();
        validate_intrinsic_bindings(&env).expect("default ValueEnv should validate");
    }

    #[test]
    fn validate_intrinsic_bindings_reports_arity_mismatch() {
        let mut env = ValueEnv::new();
        let mut sig = env
            .get_function("Vector.append")
            .expect("Vector.append should be registered")
            .clone();
        sig.params.pop();
        env.update_function(sig);

        let err = validate_intrinsic_bindings(&env).expect_err("shape mismatch should fail");
        let msg = err.to_string();
        assert!(msg.contains("Vector.append"), "unexpected error: {msg}");
        assert!(msg.contains("arity mismatch"), "unexpected error: {msg}");
    }

    #[test]
    fn validate_intrinsic_bindings_reports_type_mismatch() {
        use crate::types::ty::MonoType;

        let mut env = ValueEnv::new();
        let mut sig = env
            .get_function("Int.compare")
            .expect("Int.compare should be registered")
            .clone();
        // Change first param from Int to String — should trigger type mismatch.
        sig.params[0] = MonoType::String;
        env.update_function(sig);

        let err = validate_intrinsic_bindings(&env).expect_err("type mismatch should fail");
        let msg = err.to_string();
        assert!(msg.contains("Int.compare"), "unexpected error: {msg}");
        assert!(
            msg.contains("param 0 type mismatch"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn compare_signatures_are_validated() {
        // Ensure the primitive compare intrinsics are covered by validation.
        let env = ValueEnv::new();
        for name in &[
            "Int.compare",
            "Float.compare",
            "String.compare",
            "Byte.compare",
        ] {
            assert!(
                env.get_function(name).is_some(),
                "{name} must be registered in ValueEnv for validation"
            );
        }
        validate_intrinsic_bindings(&env).expect("compare signatures should validate");
    }
}
