use std::thread;
use twinkle::interp::Interpreter;

fn run_boot_main(args: &[&str]) -> (String, String) {
    let (core_module, _registry) =
        twinkle::module::compile_entry("boot/main.tw").expect("compile boot/main.tw");

    let argv = std::iter::once("boot/main.tw".to_string())
        .chain(args.iter().map(|arg| (*arg).to_string()))
        .collect::<Vec<_>>();

    let mut interp = Interpreter::new_with_argv(core_module, Vec::<u8>::new(), argv);
    interp.run().expect("boot/main.tw should run successfully");

    let stderr_bytes = interp.error_output().to_vec();
    let stdout = String::from_utf8(interp.into_output()).expect("stdout should be valid UTF-8");
    let stderr = String::from_utf8(stderr_bytes).expect("stderr should be valid UTF-8");
    (stdout, stderr)
}

fn run_with_large_stack(f: impl FnOnce() + Send + 'static) {
    thread::Builder::new()
        .name("boot-ir-cli".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(f)
        .expect("spawn test thread")
        .join()
        .expect("join test thread");
}

fn temp_path(case: &str, ext: &str) -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("twinkle_{case}_{stamp}.{ext}"))
}

#[test]
fn ir_command_defaults_to_optimized_anf() {
    run_with_large_stack(|| {
        let (stdout, stderr) = run_boot_main(&["ir", "tests/run/hello.tw"]);

        assert!(stderr.is_empty(), "unexpected stderr: {stderr}");
        assert!(
            stdout.contains("Optimized ANF IR"),
            "default ir output should select optimized ANF:\n{stdout}"
        );
        assert!(
            stdout.contains("call Fn1(\"hello, Twinkle!\")"),
            "optimized ANF dump should include the println call:\n{stdout}"
        );
        assert!(
            !stdout.contains("Core IR"),
            "default ir output should not print extra stages:\n{stdout}"
        );
    });
}

#[test]
fn ir_command_all_prints_all_stages() {
    run_with_large_stack(|| {
        let (stdout, stderr) = run_boot_main(&["ir", "--all", "tests/run/hello.tw"]);

        assert!(stderr.is_empty(), "unexpected stderr: {stderr}");
        for header in [
            "Core IR",
            "Monomorphized Core IR",
            "ANF IR",
            "Optimized ANF IR",
            "Linked WAT",
        ] {
            assert!(
                stdout.contains(header),
                "missing stage header {header:?} in output:\n{stdout}"
            );
        }
    });
}

#[test]
fn ir_command_core_flag_handles_option_boundary_fixture() {
    run_with_large_stack(|| {
        let (stdout, stderr) =
            run_boot_main(&["ir", "--core", "tests/run/option_boundary_call.tw"]);

        assert!(stderr.is_empty(), "unexpected stderr: {stderr}");
        assert!(
            stdout.contains("fn show [FuncId(") && stdout.contains("Match : Void"),
            "core dump should include the option match body:\n{stdout}"
        );
    });
}

#[test]
fn ir_command_wat_flag_prints_module() {
    run_with_large_stack(|| {
        let (stdout, stderr) = run_boot_main(&["ir", "--wat", "tests/run/hello.tw"]);

        assert!(stderr.is_empty(), "unexpected stderr: {stderr}");
        assert!(
            stdout.contains("Linked WAT"),
            "wat output should include the stage header:\n{stdout}"
        );
        assert!(
            stdout.contains("(module") && stdout.contains("call $rt_core__println"),
            "wat output should contain the final linked module:\n{stdout}"
        );
    });
}

#[test]
fn ir_command_handles_multi_module_fixture() {
    run_with_large_stack(|| {
        let (stdout, stderr) = run_boot_main(&["ir", "--core", "tests/run/multi_module/main.tw"]);

        assert!(stderr.is_empty(), "unexpected stderr: {stderr}");
        assert!(
            stdout.contains("fn make [FuncId(") && stdout.contains("fn __init__ [FuncId("),
            "core dump should include linked dependency and entry init:\n{stdout}"
        );
    });
}

#[test]
fn check_command_handles_multi_module_fixture() {
    run_with_large_stack(|| {
        let (stdout, stderr) = run_boot_main(&["check", "tests/run/multi_module/main.tw"]);

        assert!(stderr.is_empty(), "unexpected stderr: {stderr}");
        assert!(
            stdout.contains("Type checking succeeded: tests/run/multi_module/main.tw"),
            "check output should report success:\n{stdout}"
        );
    });
}

#[test]
fn build_command_writes_wat_for_multi_module_fixture() {
    run_with_large_stack(|| {
        let out = temp_path("boot_build_multi_module", "wat");
        let out_text = out.to_string_lossy().into_owned();

        let (stdout, stderr) =
            run_boot_main(&["build", "tests/run/multi_module/main.tw", "-o", &out_text]);

        assert!(stderr.is_empty(), "unexpected stderr: {stderr}");
        assert!(
            stdout.contains("WAT output:"),
            "build output should report the written path:\n{stdout}"
        );

        let wat = std::fs::read_to_string(&out).expect("boot build should write wat output");
        assert!(
            wat.contains("(module") && wat.contains("(start $user__"),
            "written wat should look like a linked user module:\n{wat}"
        );

        let (program_stdout, program_stderr) =
            twinkle::cli::run_wasm::run_wasm_capture(&out_text).expect("host should run built wat");
        let _ = std::fs::remove_file(&out);

        assert_eq!(
            program_stderr, "",
            "unexpected runtime stderr: {program_stderr}"
        );
        assert!(
            program_stdout.contains("0\n3\n4\n3\n25\n"),
            "running the built wat should preserve program behavior:\n{program_stdout}"
        );
    });
}
