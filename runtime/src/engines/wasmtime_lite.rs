//! Minimal wasmtime-based engine for host testing (std only).
//! Not intended for microcontrollers; enables a fast host path for integration.

use crate::{Engine, Error, ModuleId, Result};
use std::collections::HashMap;
use wasmtime::{Engine as HostEngine, Instance, Module, Store};

/// wasmtime-backed engine (host-only).
pub struct WasmtimeLiteEngine {
    engine: HostEngine,
    modules: HashMap<ModuleId, Module>,
}

impl WasmtimeLiteEngine {
    pub fn new() -> Result<Self> {
        let mut config = wasmtime::Config::new();
        config.cranelift_opt_level(wasmtime::OptLevel::Speed);
        let engine = HostEngine::new(&config).map_err(|_| Error::Engine("wasmtime init"))?;
        Ok(Self {
            engine,
            modules: HashMap::new(),
        })
    }
}

impl Engine for WasmtimeLiteEngine {
    type ModuleHandle = ModuleId;
    type Context = ();

    fn load(&mut self, id: ModuleId, module: &[u8]) -> Result<Self::ModuleHandle> {
        if module.is_empty() {
            return Err(Error::Engine("wasmtime: empty module"));
        }
        let compiled = Module::from_binary(&self.engine, module)
            .map_err(|_| Error::Engine("wasmtime compile"))?;
        self.modules.insert(id, compiled);
        Ok(id)
    }

    fn invoke(
        &mut self,
        handle: Self::ModuleHandle,
        entry: &str,
        _ctx: &mut Self::Context,
    ) -> Result<()> {
        let module = self.modules.get(&handle).ok_or(Error::ModuleNotFound)?;
        let mut store = Store::new(&self.engine, ());
        let instance = Instance::new(&mut store, module, &[])
            .map_err(|_| Error::Engine("wasmtime instantiate"))?;
        let func = instance
            .get_typed_func::<(), ()>(&mut store, entry)
            .map_err(|_| Error::EntryNotFound)?;
        func.call(&mut store, ())
            .map_err(|_| Error::Engine("wasmtime call"))?;
        Ok(())
    }
}
