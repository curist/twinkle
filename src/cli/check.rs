use anyhow::Result;

pub fn check_file(file_path: &str) -> Result<()> {
    match crate::module::check_entry(file_path) {
        Ok(_) => {
            println!("✓ Type checking succeeded: {}", file_path);
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e);
            anyhow::bail!("Check failed");
        }
    }
}
