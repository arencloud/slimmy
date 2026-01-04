#![no_std]

/// Minimal entry point for wasm3 demo: no args, no return, no imports.
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn main() {}

/// Abort-on-panic for no_std wasm builds.
#[cfg_attr(not(test), panic_handler)]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
