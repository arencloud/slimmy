#![no_std]

/// Minimal entry point for wasm3 demo: no args, no return, no imports.
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn main() {}
