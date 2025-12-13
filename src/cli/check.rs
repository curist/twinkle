use anyhow::Result;
use std::fs;

pub fn check_file(file_path: &str) -> Result<()> {
    let source = fs::read_to_string(file_path)?;

    // Parse (returns AST + FileRegistry)
    let (ast, file_registry) = crate::syntax::parse_source(&source, file_path)?;

    // Resolve names
    let (type_env, value_env) = match crate::types::Resolver::resolve(&ast) {
        Ok(envs) => envs,
        Err(errors) => {
            for error in &errors {
                // Note: During name resolution, TypeEnv is not yet complete
                // So we pass None for type_env, which shows Type#<id> instead of names
                // This is acceptable for name resolution errors
                eprintln!("{}", error.format(&file_registry, None));
            }
            anyhow::bail!("Name resolution failed with {} errors", errors.len());
        }
    };

    // Type check
    match crate::types::TypeChecker::check_module(&ast, type_env.clone(), value_env) {
        Ok((_type_map, _type_env)) => {
            println!("✓ Type checking succeeded: {}", file_path);
            Ok(())
        }
        Err(errors) => {
            for error in &errors {
                // Pass Some(&type_env) to show readable type names in error messages
                eprintln!("{}", error.format(&file_registry, Some(&type_env)));
            }
            anyhow::bail!("Type checking failed with {} errors", errors.len());
        }
    }
}
