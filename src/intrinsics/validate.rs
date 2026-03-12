use anyhow::{Result, bail};

use crate::types::env::ValueEnv;
use crate::types::ty::FunctionSignature;

use super::{registry, signatures};

fn compare_signature_shape(
    spec: &registry::IntrinsicSpec,
    sig: &FunctionSignature,
    expected: &signatures::IntrinsicContract,
    errors: &mut Vec<String>,
) {
    if sig.type_params.len() != expected.type_params.len() {
        errors.push(format!(
            "FuncId({}) {}: generic count mismatch (env={}, expected={})",
            spec.func_id.0,
            spec.twinkle_name,
            sig.type_params.len(),
            expected.type_params.len()
        ));
    }
    if sig.params.len() != expected.params.len() {
        errors.push(format!(
            "FuncId({}) {}: arity mismatch (env={}, expected={})",
            spec.func_id.0,
            spec.twinkle_name,
            sig.params.len(),
            expected.params.len()
        ));
    }
}

/// Validate intrinsic bindings against the current ValueEnv signatures.
///
/// Transitional behavior:
/// - Every `IntrinsicSpec` entry is checked (no silent skips).
/// - Signature registry entries must be present in `ValueEnv`.
/// - Where a Rust-side contract exists, coarse shape must match
///   (`type_params.len`, `params.len`).
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
    fn validate_intrinsic_bindings_reports_shape_mismatch() {
        let mut env = ValueEnv::new();
        let mut sig = env
            .get_function("Vector.push")
            .expect("Vector.push should be registered")
            .clone();
        sig.params.pop();
        env.update_function(sig);

        let err = validate_intrinsic_bindings(&env).expect_err("shape mismatch should fail");
        let msg = err.to_string();
        assert!(msg.contains("Vector.push"), "unexpected error: {msg}");
        assert!(msg.contains("arity mismatch"), "unexpected error: {msg}");
    }
}
