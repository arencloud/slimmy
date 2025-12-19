#![cfg_attr(not(feature = "std"), no_std)]

// Minimal runtime harness for OTA-delivered WebAssembly modules.

#[cfg(feature = "alloc")]
extern crate alloc;

use core::fmt;

/// Opaque identifier for a module stored on the device.
pub type ModuleId = u32;

/// Result alias used by the runtime.
pub type Result<T> = core::result::Result<T, Error>;

/// Common error cases for the runtime and engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The requested module is not present in the current store.
    ModuleNotFound,
    /// The runtime could not find or invoke the requested entry point.
    EntryNotFound,
    /// The underlying engine failed. Message kept as &'static str to stay tiny.
    Engine(&'static str),
    /// The operation is not supported by the current configuration.
    Unsupported,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ModuleNotFound => f.write_str("module not found"),
            Error::EntryNotFound => f.write_str("entry not found"),
            Error::Engine(msg) => f.write_str(msg),
            Error::Unsupported => f.write_str("operation not supported"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

/// Source of WASM bytecode.
pub trait ModuleSource {
    /// Fetches raw bytes for a module id. Returned slice must stay valid for the
    /// duration of the call to the engine.
    fn fetch(&self, id: ModuleId) -> Option<&[u8]>;
}

/// Execution engine abstraction so the runtime can swap wasm3 / WAMR / etc.
pub trait Engine {
    /// Handle to a loaded module inside the engine.
    type ModuleHandle: Copy;
    /// Optional per-execution context (can be `()` when not needed).
    type Context;

    /// Prepares a module for execution.
    fn load(&mut self, id: ModuleId, module: &[u8]) -> Result<Self::ModuleHandle>;

    /// Invokes an exported function by name.
    fn invoke(
        &mut self,
        handle: Self::ModuleHandle,
        entry: &str,
        ctx: &mut Self::Context,
    ) -> Result<()>;

    /// Optional cleanup hook; default is a no-op.
    fn drop_module(&mut self, _handle: Self::ModuleHandle) {}
}

/// Minimal runtime that orchestrates loading and invoking modules.
pub struct Runtime<E, S> {
    engine: E,
    source: S,
}

pub mod engines;
pub mod storage;
pub mod manifest;

impl<E, S> Runtime<E, S>
where
    E: Engine,
    S: ModuleSource,
{
    /// Creates a runtime from an engine and a module source.
    pub const fn new(engine: E, source: S) -> Self {
        Self { engine, source }
    }

    /// Loads and runs a module entry point.
    pub fn execute(&mut self, module_id: ModuleId, entry: &str, ctx: &mut E::Context) -> Result<()> {
        let module_bytes = self
            .source
            .fetch(module_id)
            .ok_or(Error::ModuleNotFound)?;
        let handle = self.engine.load(module_id, module_bytes)?;
        self.engine.invoke(handle, entry, ctx)
    }

    /// Mutable access to the engine for fine-grained control (e.g., configuring imports).
    pub fn engine(&mut self) -> &mut E {
        &mut self.engine
    }

    /// Access to the module source.
    pub fn source(&self) -> &S {
        &self.source
    }

    /// Consumes the runtime and returns its parts.
    pub fn into_parts(self) -> (E, S) {
        (self.engine, self.source)
    }
}

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

/// Simple in-memory module store for devices that have alloc support.
#[cfg(feature = "alloc")]
pub struct MemoryStore {
    modules: Vec<(ModuleId, Vec<u8>)>,
}

#[cfg(feature = "alloc")]
impl MemoryStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self { modules: Vec::new() }
    }

    /// Inserts or replaces a module.
    pub fn upsert(&mut self, id: ModuleId, bytes: impl Into<Vec<u8>>) {
        let bytes = bytes.into();
        if let Some((_, existing)) = self.modules.iter_mut().find(|(stored_id, _)| *stored_id == id)
        {
            *existing = bytes;
        } else {
            self.modules.push((id, bytes));
        }
    }

    /// Clears all modules, useful when reclaiming RAM.
    pub fn clear(&mut self) {
        self.modules.clear();
    }
}

#[cfg(feature = "alloc")]
impl ModuleSource for MemoryStore {
    fn fetch(&self, id: ModuleId) -> Option<&[u8]> {
        self.modules
            .iter()
            .find(|(stored_id, _)| *stored_id == id)
            .map(|(_, bytes)| bytes.as_slice())
    }
}

/// Caches module handles inside the engine to avoid re-loading.
#[cfg(feature = "alloc")]
pub struct CachedEngine<E>
where
    E: Engine,
    E::ModuleHandle: PartialEq,
{
    inner: E,
    cache: Vec<(ModuleId, E::ModuleHandle)>,
}

#[cfg(feature = "alloc")]
impl<E> CachedEngine<E>
where
    E: Engine,
    E::ModuleHandle: PartialEq,
{
    /// Wraps an engine with a small cache.
    pub fn new(inner: E) -> Self {
        Self {
            inner,
            cache: Vec::new(),
        }
    }

    fn cached_handle(&self, id: ModuleId) -> Option<E::ModuleHandle> {
        self.cache
            .iter()
            .find(|(cached_id, _)| *cached_id == id)
            .map(|(_, handle)| *handle)
    }

    /// Drops the cached handle if present and forwards to the inner engine.
    pub fn drop_cached(&mut self, handle: E::ModuleHandle) {
        if let Some(pos) = self.cache.iter().position(|(_, h)| *h == handle) {
            self.cache.swap_remove(pos);
        }
        self.inner.drop_module(handle);
    }

    /// Returns the wrapped engine, discarding the cache.
    pub fn into_inner(self) -> E {
        self.inner
    }
}

#[cfg(feature = "alloc")]
impl<E> Engine for CachedEngine<E>
where
    E: Engine,
    E::ModuleHandle: PartialEq,
{
    type ModuleHandle = E::ModuleHandle;
    type Context = E::Context;

    fn load(&mut self, id: ModuleId, module: &[u8]) -> Result<Self::ModuleHandle> {
        if let Some(handle) = self.cached_handle(id) {
            return Ok(handle);
        }

        let handle = self.inner.load(id, module)?;
        self.cache.push((id, handle));
        Ok(handle)
    }

    fn invoke(
        &mut self,
        handle: Self::ModuleHandle,
        entry: &str,
        ctx: &mut Self::Context,
    ) -> Result<()> {
        self.inner.invoke(handle, entry, ctx)
    }

    fn drop_module(&mut self, handle: Self::ModuleHandle) {
        self.drop_cached(handle);
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::string::String;
    use std::vec::Vec;

    #[derive(Default)]
    struct MockEngine {
        loaded: HashMap<ModuleId, usize>,
        invoked: Vec<(ModuleId, String)>,
    }

    impl Engine for MockEngine {
        type ModuleHandle = ModuleId;
        type Context = ();

        fn load(&mut self, id: ModuleId, module: &[u8]) -> Result<Self::ModuleHandle> {
            // A zero-length module is treated as invalid.
            if module.is_empty() {
                return Err(Error::Engine("empty module"));
            }

            let entry = self.loaded.entry(id).or_insert(0);
            *entry += 1;
            Ok(id)
        }

        fn invoke(
            &mut self,
            handle: Self::ModuleHandle,
            entry: &str,
            _ctx: &mut Self::Context,
        ) -> Result<()> {
            self.invoked.push((handle, entry.to_string()));
            Ok(())
        }
    }

    impl ModuleSource for HashMap<ModuleId, Vec<u8>> {
        fn fetch(&self, id: ModuleId) -> Option<&[u8]> {
            self.get(&id).map(|bytes| bytes.as_slice())
        }
    }

    #[test]
    fn loads_and_invokes_module() {
        let mut modules = HashMap::new();
        modules.insert(1, vec![1, 2, 3]);

        let engine = MockEngine::default();
        let mut runtime = Runtime::new(engine, modules);

        runtime.execute(1, "tick", &mut ()).unwrap();

        let (engine, _) = runtime.into_parts();
        assert_eq!(engine.loaded.get(&1), Some(&1));
        assert_eq!(engine.invoked.len(), 1);
    }

    #[test]
    fn cached_engine_avoids_reloading() {
        let mut store = MemoryStore::new();
        store.upsert(7, vec![0xAA, 0xBB, 0xCC]);

        let engine = MockEngine::default();
        let mut runtime = Runtime::new(CachedEngine::new(engine), store);

        runtime.execute(7, "start", &mut ()).unwrap();
        runtime.execute(7, "start", &mut ()).unwrap();

        let (engine, _) = runtime.into_parts();
        let engine = engine.into_inner();
        assert_eq!(engine.loaded.get(&7), Some(&1));
        assert_eq!(engine.invoked.len(), 2);
    }

    #[test]
    fn missing_module_returns_error() {
        let mut runtime = Runtime::new(MockEngine::default(), HashMap::<ModuleId, Vec<u8>>::new());
        let err = runtime.execute(42, "entry", &mut ()).unwrap_err();
        assert_eq!(err, Error::ModuleNotFound);
    }
}
