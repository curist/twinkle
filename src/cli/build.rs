use anyhow::Result;

pub fn build_file(file_path: &str, output: Option<&str>) -> Result<()> {
    println!("Building: {}", file_path);
    if let Some(out) = output {
        println!("Output: {}", out);
    }
    println!("(Codegen not yet implemented - Stage 7)");
    Ok(())
}
