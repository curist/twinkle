use twinkle::types::ty::MonoType;

fn fixture(name: &str) -> String {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/run")
        .join(name)
        .to_string_lossy()
        .to_string()
}

fn has_type_vars(ty: &MonoType) -> bool {
    match ty {
        MonoType::Var(_) | MonoType::MetaVar(_) => true,
        MonoType::Vector(inner) => has_type_vars(inner),
        MonoType::Dict(key, value) => has_type_vars(key) || has_type_vars(value),
        MonoType::Function { params, ret } => {
            params.iter().any(has_type_vars) || has_type_vars(ret)
        }
        MonoType::Named { args, .. } => args.iter().any(has_type_vars),
        MonoType::Int
        | MonoType::Float
        | MonoType::Bool
        | MonoType::Byte
        | MonoType::String
        | MonoType::Void
        | MonoType::Never
        | MonoType::ExternRef(_) => false,
    }
}

#[test]
fn backend_anf_pipeline_monomorphizes_generic_fixture() {
    let pipeline =
        twinkle::backend_pipeline::compile_backend_anf(&fixture("generic_user_funcs.tw"))
            .expect("backend pipeline compile should succeed");

    let names = pipeline
        .anf_module
        .functions
        .iter()
        .map(|func| func.name.as_str())
        .collect::<Vec<_>>();

    assert!(names.contains(&"generic_user_funcs.id__Int"));
    assert!(names.contains(&"generic_user_funcs.id__String"));
    assert!(names.contains(&"generic_user_funcs.id__Bool"));
    assert!(names.contains(&"generic_user_funcs.apply__Int_Int"));
    assert!(names.contains(&"generic_user_funcs.first__String_Bool"));
    assert!(!names.contains(&"id"));
    assert!(!names.contains(&"apply"));
    assert!(!names.contains(&"first"));

    for func in &pipeline.anf_module.functions {
        for ty in &func.param_tys {
            assert!(
                !has_type_vars(ty),
                "param type for '{}' should be monomorphized: {ty:?}",
                func.name
            );
        }
        assert!(
            !has_type_vars(&func.return_ty),
            "return type for '{}' should be monomorphized: {:?}",
            func.name,
            func.return_ty
        );
    }
}

#[test]
fn backend_opt_pipeline_keeps_monomorphized_generic_fixture() {
    let pipeline =
        twinkle::backend_pipeline::compile_backend_opt(&fixture("generic_user_funcs.tw"))
            .expect("backend opt pipeline compile should succeed");

    let names = pipeline
        .optimized_anf_module
        .functions
        .iter()
        .map(|func| func.name.as_str())
        .collect::<Vec<_>>();

    assert!(names.contains(&"generic_user_funcs.id__Int"));
    assert!(names.contains(&"generic_user_funcs.apply__Int_Int"));
    assert!(!names.contains(&"id"));
    assert!(!names.contains(&"apply"));
}
