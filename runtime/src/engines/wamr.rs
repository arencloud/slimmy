//! WAMR engine placeholder (still unsupported). Replace with a real integration when stable bindings are available.
use crate::{Engine, Error, ModuleId, Result};

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
