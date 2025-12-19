//! Minimal wasm3-based engine implementation.
//! Intended for host/tests and small targets that can link the interpreter.

use alloc::vec::Vec;
use wasm3::error::Error as Wasm3Error;
use wasm3::{Environment, Runtime as M3Runtime};

use crate::{Engine, Error, ModuleId, Result};

/// Default stack size in "slots" (4 bytes each). 4 KiB is typically enough for tiny modules.
pub const DEFAULT_STACK_SLOTS: u32 = 1024;

/// wasm3-backed engine that reloads the module for each invocation.
///
/// This keeps lifetimes simple and is still fast for small modules. Pair with
/// `CachedEngine` to avoid repeated load costs when desired.
pub struct Wasm3Engine {
    env: Environment,
    stack_slots: u32,
    modules: Vec<(ModuleId, Vec<u8>)>,
}

impl Wasm3Engine {
    /// Constructs a new engine with the provided stack size (in slots).
    pub fn new(stack_slots: u32) -> Result<Self> {
        let env = Environment::new().map_err(map_err)?;
        Ok(Self {
            env,
            stack_slots,
            modules: Vec::new(),
        })
    }

    /// Replaces or inserts a module's bytes.
    fn upsert_module(&mut self, id: ModuleId, bytes: Vec<u8>) {
        if let Some((_, existing)) = self.modules.iter_mut().find(|(mid, _)| *mid == id) {
            *existing = bytes;
        } else {
            self.modules.push((id, bytes));
        }
    }

    fn module_bytes(&self, id: ModuleId) -> Result<&[u8]> {
        self.modules
            .iter()
            .find(|(mid, _)| *mid == id)
            .map(|(_, bytes)| bytes.as_slice())
            .ok_or(Error::ModuleNotFound)
    }
}

impl Engine for Wasm3Engine {
    type ModuleHandle = ModuleId;
    type Context = ();

    fn load(&mut self, id: ModuleId, module: &[u8]) -> Result<Self::ModuleHandle> {
        if module.is_empty() {
            return Err(Error::Engine("wasm3: empty module"));
        }

        // wasm3 keeps a copy of the bytes, so store them for reloading on invoke.
        self.upsert_module(id, module.to_vec());
        Ok(id)
    }

    fn invoke(
        &mut self,
        handle: Self::ModuleHandle,
        entry: &str,
        _ctx: &mut Self::Context,
    ) -> Result<()> {
        let bytes = self.module_bytes(handle)?;

        let runtime = M3Runtime::new(&self.env, self.stack_slots).map_err(map_err)?;
        let module = runtime
            .parse_and_load_module(bytes.to_vec())
            .map_err(map_err)?;

        // Functions with no args/returns keep the footprint minimal for now.
        let func: wasm3::Function<(), ()> = module.find_function(entry).map_err(map_err)?;
        func.call().map_err(map_err)?;
        Ok(())
    }
}

fn map_err(err: Wasm3Error) -> Error {
    match err {
        Wasm3Error::FunctionNotFound => Error::EntryNotFound,
        Wasm3Error::ModuleNotFound => Error::ModuleNotFound,
        Wasm3Error::ModuleLoadEnvMismatch => Error::Engine("wasm3: env mismatch"),
        Wasm3Error::InvalidFunctionSignature => Error::Engine("wasm3: invalid signature"),
        Wasm3Error::Wasm3(inner) if inner.is_trap(wasm3::error::Trap::StackOverflow) => {
            Error::Engine("wasm3: stack overflow")
        }
        Wasm3Error::Wasm3(_) => Error::Engine("wasm3: runtime error"),
    }
}
