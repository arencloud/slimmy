use clap::Parser;
use ed25519_dalek::Signer;
use runtime::manifest::{
    encode, signing_preimage, FLAG_REQUIRE_SIGNATURE, FLAG_ROLLBACK_PROTECTED,
};
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "packer",
    about = "Bundle a WASM module into a signed manifest blob."
)]
struct Args {
    /// Path to the input .wasm module
    #[arg(value_name = "MODULE")]
    module: PathBuf,

    /// Module id to embed in the manifest
    #[arg(long, default_value_t = 1)]
    module_id: u32,

    /// Entrypoint name
    #[arg(long, default_value = "main")]
    entry: String,

    /// Output file path
    #[arg(short, long)]
    out: Option<PathBuf>,

    /// Optional hex-encoded 32-byte Ed25519 secret key to sign the blob
    #[arg(long, value_name = "HEX32")]
    sign_key_hex: Option<String>,

    /// Require signature flag in manifest (fails if no signing key provided)
    #[arg(long, default_value_t = false)]
    require_signature: bool,

    /// Monotonic sequence for rollback protection (sets rollback flag when >0)
    #[arg(long, default_value_t = 0)]
    sequence: u32,

    /// Pad module to the next multiple of this many bytes (useful for flash erase blocks)
    #[arg(long, value_name = "N")]
    pad_to: Option<usize>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut module_bytes = fs::read(&args.module)?;
    if let Some(block) = args.pad_to {
        if block == 0 {
            return Err("pad_to must be > 0".into());
        }
        let padded = pad_to(module_bytes.len(), block);
        if padded > module_bytes.len() {
            module_bytes.resize(padded, 0xFF);
        }
    }

    if args.require_signature && args.sign_key_hex.is_none() {
        return Err("require_signature set but no signing key provided".into());
    }

    let mut flags = 0u8;
    if args.require_signature || args.sign_key_hex.is_some() {
        flags |= FLAG_REQUIRE_SIGNATURE;
    }
    if args.sequence > 0 {
        flags |= FLAG_ROLLBACK_PROTECTED;
    }

    let signature = if let Some(hex_key) = args.sign_key_hex.as_deref() {
        let key_bytes = parse_hex_key(hex_key)?;
        let signing = ed25519_dalek::SigningKey::from_bytes(&key_bytes);

        let preimage = signing_preimage(
            args.module_id,
            &args.entry,
            &module_bytes,
            flags,
            args.sequence,
        )
        .map_err(to_io_error)?;
        let sig = signing.sign(&preimage).to_bytes();
        Some(sig)
    } else {
        None
    };

    let blob = encode(
        args.module_id,
        &args.entry,
        &module_bytes,
        flags,
        args.sequence,
        signature,
    )
    .map_err(to_io_error)?;

    let out_path = args
        .out
        .unwrap_or_else(|| default_out_path(&args.module, signature.is_some()));
    fs::write(&out_path, blob)?;

    println!(
        "✅ packed module: id={} entry={} signed={} seq={} flags=0x{:02x} len={} -> {}",
        args.module_id,
        args.entry,
        signature.is_some(),
        args.sequence,
        flags,
        module_bytes.len(),
        out_path.display()
    );

    Ok(())
}

fn parse_hex_key(hex: &str) -> Result<[u8; 32], io::Error> {
    let bytes = hex::decode(hex.trim())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "sign_key_hex not valid hex"))?;
    let arr: [u8; 32] = bytes.try_into().map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "sign_key_hex must be 32 bytes")
    })?;
    Ok(arr)
}

fn default_out_path(input: &PathBuf, signed: bool) -> PathBuf {
    let mut out = input.clone();
    out.set_extension(if signed { "smny.sig" } else { "smny" });
    out
}

fn to_io_error(err: runtime::Error) -> io::Error {
    io::Error::new(io::ErrorKind::Other, format!("manifest error: {err}"))
}

fn pad_to(len: usize, block: usize) -> usize {
    if block == 0 {
        len
    } else {
        ((len + block - 1) / block) * block
    }
}

#[cfg(test)]
mod tests {
    use super::pad_to;

    #[test]
    fn pad_rounds_up() {
        assert_eq!(pad_to(0, 4096), 0);
        assert_eq!(pad_to(1, 4), 4);
        assert_eq!(pad_to(4, 4), 4);
        assert_eq!(pad_to(5, 4), 8);
    }
}
