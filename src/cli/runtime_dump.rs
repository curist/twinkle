use crate::runtime;
use crate::wasm::emit::emit_wat;
use crate::wasm::linker::{LinkError, link};
use anyhow::Result;

pub fn runtime_dump() -> Result<()> {
    let modules = runtime::all_modules();
    let linked = link(modules, None).map_err(|errs: Vec<LinkError>| {
        let msgs: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
        anyhow::anyhow!("link errors:\n{}", msgs.join("\n"))
    })?;
    println!("{}", emit_wat(&linked));
    Ok(())
}
