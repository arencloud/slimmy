//! Minimal WAMR interpreter-mode stub. Uses the C API via libc calls.
//! This is intentionally small and avoids features beyond basic load/call.

use crate::{Engine, Error, ModuleId, Result};

/// Minimal WAMR interpreter engine (placeholder).
pub struct WamrEngine;

impl WamrEngine {
    pub fn new() -> Self {
        Self
    }
}

impl Engine for WamrEngine {
    type ModuleHandle = ModuleId;
    type Context = ();

    fn load(&mut self, _id: ModuleId, _module: &[u8]) -> Result<Self::ModuleHandle> {
        Err(Error::Unsupported)
    }

    fn invoke(
        &mut self,
        _handle: Self::ModuleHandle,
        _entry: &str,
        _ctx: &mut Self::Context,
    ) -> Result<()> {
        Err(Error::Unsupported)
    }
}
