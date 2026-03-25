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
