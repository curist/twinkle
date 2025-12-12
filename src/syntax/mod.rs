// Lexer, Parser, AST - Stage 1

use anyhow::Result;

/// Stub parser for Stage 0 testing
/// Returns Ok(()) for valid-looking input, Err for obvious syntax errors
pub fn parse(source: &str) -> Result<()> {
    // Basic stub validation - just check for balanced braces
    let open_braces = source.chars().filter(|&c| c == '{').count();
    let close_braces = source.chars().filter(|&c| c == '}').count();

    if open_braces != close_braces {
        anyhow::bail!("Unbalanced braces: {} open, {} close", open_braces, close_braces);
    }

    // For now, everything else "parses"
    Ok(())
}
