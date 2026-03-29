use std::path::PathBuf;

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/run")
        .join(name)
        .to_string_lossy()
        .to_string()
}

fn build_wat(file_path: &str) -> String {
    twinkle::cli::build::build_wat(file_path).expect("build_wat failed")
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
    let bootlib_calls = count_substring_in_user_funcs(&wat, "call $bootlib_vector_i64__func_");
    let slice_calls = count_substring_in_user_funcs(&wat, "call $rt_arr__slice_i64");
    assert!(
        bootlib_calls > 0,
        "expected range collect to route through bootlib.vector_i64 in user funcs"
    );
    assert!(
        slice_calls == 0,
        "expected range collect compaction to stay behind the bootlib.vector_i64 boundary"
    );
}

#[test]
fn vector_collect_uses_builder_push_not_concat() {
    let wat = build_wat(&fixture("collect.tw"));
    let concat_calls = count_substring_in_user_funcs(&wat, "call $rt_arr__concat");
    let bootlib_calls = count_substring_in_user_funcs(&wat, "call $bootlib_vector_i64__func_");
    assert_eq!(
        concat_calls, 0,
        "vector collect should not use concat growth in user funcs"
    );
    assert!(
        bootlib_calls > 0,
        "vector collect should route through bootlib.vector_i64 in user funcs"
    );
}

#[test]
fn vector_collect_int_uses_i64_builder_family() {
    let wat = build_wat(&fixture("collect.tw"));
    let bootlib_calls = count_substring_in_user_funcs(&wat, "call $bootlib_vector_i64__func_");
    let typed_push_calls = count_substring_in_user_funcs(&wat, "call $rt_arr__builder_push_i64");
    let typed_freeze_calls =
        count_substring_in_user_funcs(&wat, "call $rt_arr__builder_freeze_i64");
    assert!(
        bootlib_calls > 0,
        "Vector<Int> collect should route through bootlib.vector_i64 in user funcs"
    );
    assert!(
        typed_push_calls == 0,
        "Vector<Int> collect should not call rt_arr__builder_push_i64 directly in user funcs"
    );
    assert!(
        typed_freeze_calls == 0,
        "Vector<Int> collect should not call rt_arr__builder_freeze_i64 directly in user funcs"
    );
}

#[test]
fn iterator_collect_uses_builder_push_not_concat() {
    let wat = build_wat(&fixture("iterator.tw"));
    let concat_calls = count_substring_in_user_funcs(&wat, "call $rt_arr__concat");
    let bootlib_calls = count_substring_in_user_funcs(&wat, "call $bootlib_vector_i64__func_");
    assert_eq!(
        concat_calls, 0,
        "iterator collect should not use concat growth in user funcs"
    );
    assert!(
        bootlib_calls > 0,
        "iterator collect should route through bootlib.vector_i64 in user funcs"
    );
}

#[test]
fn vector_methods_int_use_i64_helper_family() {
    let wat = build_wat(&fixture("vector_methods.tw"));
    assert!(
        count_substring_in_user_funcs(&wat, "call $bootlib_vector_i64__func_") > 0,
        "Vector<Int> methods should route through bootlib.vector_i64 in user funcs"
    );
    assert!(
        count_substring_in_user_funcs(&wat, "call $rt_arr__len_i64") == 0,
        "Vector<Int>.len should not call rt_arr__len_i64 directly in user funcs"
    );
    assert!(
        count_substring_in_user_funcs(&wat, "call $rt_arr__concat_i64") == 0,
        "Vector<Int>.concat should not call rt_arr__concat_i64 directly in user funcs"
    );
    assert!(
        count_substring_in_user_funcs(&wat, "call $rt_arr__slice_i64") == 0,
        "Vector<Int>.slice should not call rt_arr__slice_i64 directly in user funcs"
    );
    assert!(
        count_substring_in_user_funcs(&wat, "call $rt_arr__make_i64") == 0,
        "Vector<Int>.make should not call rt_arr__make_i64 directly in user funcs"
    );
    assert!(
        count_substring_in_user_funcs(&wat, "call $rt_arr__set_i64") == 0,
        "Vector<Int>.set should not call rt_arr__set_i64 directly in user funcs"
    );
}
