pub mod types;
pub mod arr;
pub mod str;
pub mod dict;
pub mod core;

use crate::wasm::ir::ModuleIR;

/// Return all runtime modules in dependency order (types first).
pub fn all_modules() -> Vec<ModuleIR> {
    vec![
        types::make(),
        arr::make(),
        self::str::make(),
        dict::make(),
        core::make(),
    ]
}
