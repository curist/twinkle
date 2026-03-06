use std::collections::HashMap;
use std::path::PathBuf;

use twinkle::codegen::emit::emit_user_module;
use twinkle::ir::lower_anf::lower_module;
use twinkle::opt::optimize_module;
use twinkle::runtime;
use twinkle::wasm::{emit::emit_wat, linker::link};

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/run")
        .join(name)
        .to_string_lossy()
        .to_string()
}

fn build_wat(file_path: &str) -> String {
    let (core_module, _) = twinkle::module::compile_entry(file_path).expect("compile failed");
    let anf = lower_module(&core_module);
    let optimized = optimize_module(anf);
    let user_module = emit_user_module(&optimized, &core_module.type_env, &HashMap::new());
    let mut modules = runtime::all_modules();
    modules.push(user_module);
    let linked = link(modules, None).expect("link failed");
    emit_wat(&linked)
}

fn count_substring_in_user_funcs(wat: &str, needle: &str) -> usize {
    let mut in_user = false;
    let mut depth: i32 = 0;
    let mut count = 0;

    for line in wat.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("(func") && trimmed.contains("$user__func_") {
            in_user = true;
            depth = 0;
        }
        if in_user {
            for ch in trimmed.chars() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            in_user = false;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if in_user && trimmed.contains(needle) {
                count += 1;
            }
        }
    }

    count
}

#[test]
fn range_collect_avoids_concat_growth_in_user_funcs() {
    let wat = build_wat(&fixture("fold_small.tw"));
    let concat_calls = count_substring_in_user_funcs(&wat, "call $rt_arr__concat");
    assert_eq!(
        concat_calls, 0,
        "range collect should not use concat growth in user funcs"
    );
}

#[test]
fn range_collect_compacts_with_single_slice() {
    let wat = build_wat(&fixture("fold_small.tw"));
    let slice_calls = count_substring_in_user_funcs(&wat, "call $rt_arr__slice");
    assert!(
        slice_calls >= 1,
        "expected range collect to compact once with rt_arr__slice"
    );
}

#[test]
fn vector_collect_uses_builder_push_not_concat() {
    let wat = build_wat(&fixture("collect.tw"));
    let concat_calls = count_substring_in_user_funcs(&wat, "call $rt_arr__concat");
    let push_calls = count_substring_in_user_funcs(&wat, "call $rt_arr__builder_push");
    assert_eq!(
        concat_calls, 0,
        "vector collect should not use concat growth in user funcs"
    );
    assert!(
        push_calls > 0,
        "vector collect should use rt_arr__builder_push in user funcs"
    );
}

#[test]
fn iterator_collect_uses_builder_push_not_concat() {
    let wat = build_wat(&fixture("iterator.tw"));
    let concat_calls = count_substring_in_user_funcs(&wat, "call $rt_arr__concat");
    let push_calls = count_substring_in_user_funcs(&wat, "call $rt_arr__builder_push");
    assert_eq!(
        concat_calls, 0,
        "iterator collect should not use concat growth in user funcs"
    );
    assert!(
        push_calls > 0,
        "iterator collect should use rt_arr__builder_push in user funcs"
    );
}
