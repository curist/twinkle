use anyhow::{Context, Result};
use std::fs;

pub fn parse_file(file_path: &str) -> Result<()> {
    println!("Parsing: {}", file_path);

    let source = fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read file: {}", file_path))?;

    let (ast, _registry) = crate::syntax::parse_source(&source, file_path)
        .with_context(|| format!("Failed to parse: {}", file_path))?;

    println!("✓ Parse successful\n");
    println!("AST (pretty-printed):");
    println!("{}", crate::syntax::pretty::print_source_file(&ast));

    // Optionally show raw debug format
    if std::env::var("TWK_DEBUG_AST").is_ok() {
        println!("\nAST (debug format):");
        println!("{:#?}", ast);
    }

    Ok(())
}
