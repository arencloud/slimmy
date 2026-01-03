//! Optional engine backends.

#[cfg(feature = "engine-wasm3")]
pub mod wasm3;
#[cfg(feature = "engine-wamr")]
pub mod wamr;
#[cfg(feature = "engine-wasmtime-lite")]
pub mod wasmtime_lite;
