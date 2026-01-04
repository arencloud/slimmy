use clap::Parser;
use runtime::{manifest::Manifest, CachedEngine, MemoryStore, ModuleSource, Runtime};
#[cfg(all(feature = "wasm3", feature = "wasmtime-lite"))]
compile_error!("Select only one engine feature at a time: wasm3 or wasmtime-lite.");
#[cfg(not(any(feature = "wasm3", feature = "wasmtime-lite")))]
use runtime::{Engine, Error, ModuleId};
#[cfg(not(any(feature = "wasm3", feature = "wasmtime-lite")))]
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "host-demo", about = "Tiny host runner for slimmy modules.")]
struct Args {
    /// Path to .wasm or .smny blob
    path: PathBuf,

    /// Override entry (defaults to manifest entry when using --manifest)
    #[arg(short, long)]
    entry: Option<String>,

    /// Treat input as manifest blob (.smny/.smny.sig)
    #[arg(long)]
    manifest: bool,

    /// Hex-encoded 32-byte pubkey for signature verification (manifest mode)
    #[arg(long, value_name = "HEX32")]
    pubkey_hex: Option<String>,

    /// Require signature verification when manifest flag set
    #[arg(long)]
    require_verify: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let blob = fs::read(&args.path)?;
    let (module_bytes, entry, info) = if args.manifest {
        load_manifest_blob(&args, &blob)?
    } else {
        let entry = args.entry.unwrap_or_else(|| "main".to_string());
        (blob, entry, None)
    };

    let mut store = MemoryStore::new();
    store.upsert(1, module_bytes);

    let module_size = store.fetch(1).map(|b| b.len()).unwrap_or(0);
    let stats = run_module(store, &entry, module_size).map_err(to_io_error)?;

    if let Some((manifest, _)) = info {
        println!(
            "✅ call finished: module={} entry=`{}` bytes={} flags=0x{:02x} seq={} total_invocations={}",
            manifest.module_id,
            entry.as_str(),
            stats.last_size,
            manifest.flags,
            manifest.sequence,
            stats.invocations
        );
    } else {
        println!(
            "✅ call finished: module=1 entry=`{}` bytes={} total_invocations={}",
            entry.as_str(),
            stats.last_size,
            stats.invocations
        );
    }

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

#[cfg(not(any(feature = "wasm3", feature = "wasmtime-lite")))]
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

#[cfg(feature = "wasmtime-lite")]
fn run_module(store: MemoryStore, entry: &str, module_size: usize) -> runtime::Result<HostStats> {
    use runtime::engines::wasmtime_lite::WasmtimeLiteEngine;

    let engine = CachedEngine::new(WasmtimeLiteEngine::new()?);
    let mut runtime = Runtime::new(engine, store);

    runtime.execute(1, entry, &mut ())?;
    Ok(HostStats {
        invocations: 1,
        last_size: module_size,
    })
}

#[cfg(not(any(feature = "wasm3", feature = "wasmtime-lite")))]
fn run_module(store: MemoryStore, entry: &str, _module_size: usize) -> runtime::Result<HostStats> {
    let engine = CachedEngine::new(NoopEngine::default());
    let mut runtime = Runtime::new(engine, store);

    let mut ctx = HostStats::default();
    runtime.execute(1, entry, &mut ctx)?;
    Ok(ctx)
}

#[cfg(not(any(feature = "wasm3", feature = "wasmtime-lite")))]
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

fn load_manifest_blob<'a>(
    args: &'a Args,
    blob: &'a [u8],
) -> Result<(Vec<u8>, String, Option<(Manifest<'a>, Vec<u8>)>), Box<dyn std::error::Error>> {
    let (manifest, module) = Manifest::parse(blob).map_err(to_io_error)?;
    let module_vec = module.to_vec();

    if manifest.module_len as usize != module_vec.len() {
        return Err("manifest module_len mismatch".into());
    }

    let entry = args
        .entry
        .clone()
        .unwrap_or_else(|| manifest.entry.to_string());

    #[cfg(feature = "verify-ed25519")]
    {
        if args.require_verify && args.pubkey_hex.is_none() {
            return Err("require_verify set but no pubkey_hex provided".into());
        }
        if let Some(hex) = args.pubkey_hex.as_deref() {
            let pk = parse_hex32(hex)?;
            runtime::manifest::verify_ed25519(&manifest, &module_vec, &pk).map_err(to_io_error)?;
        }
    }

    #[cfg(not(feature = "verify-ed25519"))]
    {
        if args.require_verify || args.pubkey_hex.is_some() {
            return Err("host-demo built without verify-ed25519 feature".into());
        }
    }

    Ok((module_vec, entry, Some((manifest, module.to_vec()))))
}

#[cfg(feature = "verify-ed25519")]
fn parse_hex32(hex: &str) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    let bytes = hex::decode(hex.trim()).map_err(|_| "pubkey_hex not valid hex".to_string())?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "pubkey_hex must be 32 bytes".to_string())?;
    Ok(arr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::manifest::encode;

    #[test]
    fn loads_manifest_blob_without_verify() {
        let module = b"\x01\x02\x03";
        let entry = "main";
        let blob = encode(1, entry, module, 0, 0, None).unwrap();
        let args = Args {
            path: PathBuf::from("module.smny"),
            entry: None,
            manifest: true,
            pubkey_hex: None,
            require_verify: false,
        };

        let (out_mod, out_entry, info) = load_manifest_blob(&args, &blob).unwrap();
        assert_eq!(out_mod, module);
        assert_eq!(out_entry, entry);
        assert!(info.is_some());
    }

    #[cfg(not(feature = "verify-ed25519"))]
    #[test]
    fn require_verify_fails_without_feature() {
        let module = b"\x01\x02\x03";
        let sig = [0u8; 64];
        let blob = encode(1, "main", module, 0, 0, Some(sig)).unwrap();
        let args = Args {
            path: PathBuf::from("module.smny"),
            entry: None,
            manifest: true,
            pubkey_hex: None,
            require_verify: true,
        };

        assert!(load_manifest_blob(&args, &blob).is_err());
    }
}
