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
fn vector_collect_int_uses_i64_builder_family() {
    let wat = build_wat(&fixture("collect.tw"));
    let typed_push_calls = count_substring_in_user_funcs(&wat, "call $rt_arr__builder_push_i64");
    let typed_freeze_calls =
        count_substring_in_user_funcs(&wat, "call $rt_arr__builder_freeze_i64");
    assert!(
        typed_push_calls > 0,
        "Vector<Int> collect should use rt_arr__builder_push_i64 in user funcs"
    );
    assert!(
        typed_freeze_calls > 0,
        "Vector<Int> collect should use rt_arr__builder_freeze_i64 in user funcs"
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

#[test]
fn vector_methods_int_use_i64_helper_family() {
    let wat = build_wat(&fixture("vector_methods.tw"));
    assert!(
        count_substring_in_user_funcs(&wat, "call $rt_arr__len_i64") > 0,
        "Vector<Int>.len should use rt_arr__len_i64 in user funcs"
    );
    assert!(
        count_substring_in_user_funcs(&wat, "call $rt_arr__concat_i64") > 0,
        "Vector<Int>.concat should use rt_arr__concat_i64 in user funcs"
    );
    assert!(
        count_substring_in_user_funcs(&wat, "call $rt_arr__slice_i64") > 0,
        "Vector<Int>.slice should use rt_arr__slice_i64 in user funcs"
    );
    assert!(
        count_substring_in_user_funcs(&wat, "call $rt_arr__make_i64") > 0,
        "Vector<Int>.make should use rt_arr__make_i64 in user funcs"
    );
    assert!(
        count_substring_in_user_funcs(&wat, "call $rt_arr__get_i64") > 0,
        "Vector<Int>.get/index should use rt_arr__get_i64 in user funcs"
    );
    assert!(
        count_substring_in_user_funcs(&wat, "call $rt_arr__set_i64") > 0,
        "Vector<Int>.set should use rt_arr__set_i64 in user funcs"
    );
}
