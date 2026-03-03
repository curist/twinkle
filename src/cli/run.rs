use anyhow::Result;
use std::io::Write;

pub fn run_file(file_path: &str) -> Result<()> {
    let (core_module, _registry) =
        crate::module::compile_entry(file_path).map_err(|e| anyhow::anyhow!("{}", e))?;
    let mut interp = crate::interp::Interpreter::new(core_module, std::io::stdout());
    let result = interp.run();
    let stderr_bytes = interp.error_output();
    if !stderr_bytes.is_empty() {
        std::io::stderr().write_all(stderr_bytes).ok();
    }
    result
}
