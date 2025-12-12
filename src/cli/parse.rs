use anyhow::{Context, Result};
use std::fs;

pub fn parse_file(file_path: &str) -> Result<()> {
    println!("Parsing: {}", file_path);

    let source = fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read file: {}", file_path))?;

    crate::syntax::parse(&source)
        .with_context(|| format!("Failed to parse: {}", file_path))?;

    println!("✓ Parse successful (stub parser - Stage 1 will implement full parser)");
    Ok(())
}
