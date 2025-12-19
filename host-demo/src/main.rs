use runtime::{CachedEngine, MemoryStore, ModuleSource, Runtime};
#[cfg(not(feature = "wasm3"))]
use runtime::{Engine, Error, ModuleId};
#[cfg(not(feature = "wasm3"))]
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let module_path = match args.next() {
        Some(path) => path,
        None => {
            eprintln!("usage: host-demo <path-to-wasm> [entry]");
            return Ok(());
        }
    };
    let entry = args.next().unwrap_or_else(|| "main".to_string());

    let module_bytes = fs::read(&module_path)?;

    let mut store = MemoryStore::new();
    store.upsert(1, module_bytes);

    let module_size = store.fetch(1).map(|b| b.len()).unwrap_or(0);
    let stats = run_module(store, &entry, module_size).map_err(to_io_error)?;

    println!(
        "✅ call finished: module=1 entry=`{}` bytes={} total_invocations={}",
        entry, stats.last_size, stats.invocations
    );

    Ok(())
}

fn to_io_error(err: runtime::Error) -> io::Error {
    io::Error::new(io::ErrorKind::Other, format!("runtime error: {err}"))
}

#[derive(Default)]
struct HostStats {
    invocations: usize,
    last_size: usize,
}

#[cfg(not(feature = "wasm3"))]
#[derive(Default)]
struct NoopEngine {
    module_sizes: HashMap<ModuleId, usize>,
}

#[cfg(feature = "wasm3")]
fn run_module(store: MemoryStore, entry: &str, module_size: usize) -> runtime::Result<HostStats> {
    use runtime::engines::wasm3::{Wasm3Engine, DEFAULT_STACK_SLOTS};

    let engine = CachedEngine::new(Wasm3Engine::new(DEFAULT_STACK_SLOTS)?);
    let mut runtime = Runtime::new(engine, store);

    runtime.execute(1, entry, &mut ())?;
    Ok(HostStats {
        invocations: 1,
        last_size: module_size,
    })
}

#[cfg(not(feature = "wasm3"))]
fn run_module(store: MemoryStore, entry: &str, _module_size: usize) -> runtime::Result<HostStats> {
    let engine = CachedEngine::new(NoopEngine::default());
    let mut runtime = Runtime::new(engine, store);

    let mut ctx = HostStats::default();
    runtime.execute(1, entry, &mut ctx)?;
    Ok(ctx)
}

#[cfg(not(feature = "wasm3"))]
impl Engine for NoopEngine {
    type ModuleHandle = ModuleId;
    type Context = HostStats;

    fn load(&mut self, id: ModuleId, module: &[u8]) -> runtime::Result<Self::ModuleHandle> {
        if module.is_empty() {
            return Err(Error::Engine("module is empty"));
        }

        self.module_sizes.insert(id, module.len());
        Ok(id)
    }

    fn invoke(
        &mut self,
        handle: Self::ModuleHandle,
        entry: &str,
        ctx: &mut Self::Context,
    ) -> runtime::Result<()> {
        let size = self
            .module_sizes
            .get(&handle)
            .copied()
            .ok_or(Error::ModuleNotFound)?;

        ctx.invocations += 1;
        ctx.last_size = size;

        println!(
            "No-op engine would run module={} entry=`{}` ({} bytes)",
            handle, entry, size
        );

        Ok(())
    }
}
