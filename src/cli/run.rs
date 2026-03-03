use anyhow::Result;

pub fn run_file(file_path: &str) -> Result<()> {
    let (core_module, _registry) =
        crate::module::compile_entry(file_path).map_err(|e| anyhow::anyhow!("{}", e))?;
    let mut interp = crate::interp::Interpreter::new(core_module, std::io::stdout());
    interp.run()
}
