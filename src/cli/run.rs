use anyhow::Result;
use std::io::Write;

pub fn run_file(file_path: &str) -> Result<()> {
    run_file_with_args(file_path, &[])
}

pub fn run_file_with_args(file_path: &str, program_args: &[String]) -> Result<()> {
    let (core_module, _registry) =
        crate::module::compile_entry(file_path).map_err(|e| anyhow::anyhow!("{}", e))?;
    let mut argv = Vec::with_capacity(program_args.len() + 1);
    argv.push(file_path.to_string());
    argv.extend(program_args.iter().cloned());
    let mut interp =
        crate::interp::Interpreter::new_with_argv(core_module, std::io::stdout(), argv);
    let result = interp.run();
    let stderr_bytes = interp.error_output();
    if !stderr_bytes.is_empty() {
        std::io::stderr().write_all(stderr_bytes).ok();
    }
    result
}
