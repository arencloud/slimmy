//! Minimal manifest format and optional Ed25519 verification.
//!
//! Layout v1 (little endian):
//! - magic: 4 bytes = b"SMNY"
//! - version: u8 = 1
//! - module_id: u32
//! - module_len: u32
//! - entry_len: u8
//! - entry: [u8; entry_len] (UTF-8)
//! - signature: [u8; 64] (optional; only used when feature `verify-ed25519` is on)
//!
//! Layout v2 (default for new blobs):
//! - magic: 4 bytes = b"SMNY"
//! - version: u8 = 2
//! - module_id: u32
//! - module_len: u32
//! - flags: u8 (bit0=require signature, bit1=rollback-protected)
//! - sequence: u32 (monotonic, used with rollback flag)
//! - entry_len: u8
//! - entry: [u8; entry_len] (UTF-8)
//! - signature: [u8; 64] (optional; required if flags bit0 set)
//!
//! The signed message is the manifest bytes up to (but not including) the signature,
//! concatenated with the module bytes.

use crate::{Error, ModuleId, Result};

/// Manifest magic marker.
pub const MANIFEST_MAGIC: &[u8; 4] = b"SMNY";
/// Manifest version used for new blobs.
pub const MANIFEST_VERSION: u8 = 2;
/// Manifest version 1 (legacy).
pub const MANIFEST_VERSION_V1: u8 = 1;
/// Length of a full Ed25519 signature.
pub const SIGNATURE_LEN: usize = 64;

/// Flags bits (v2).
pub const FLAG_REQUIRE_SIGNATURE: u8 = 0b0000_0001;
pub const FLAG_ROLLBACK_PROTECTED: u8 = 0b0000_0010;

const HEADER_FIXED_V1: usize = 4 + 1 + 4 + 4 + 1;
const HEADER_FIXED_V2: usize = 4 + 1 + 4 + 4 + 1 + 4 + 1;

/// Parsed view into a manifest.
pub struct Manifest<'a> {
    pub version: u8,
    pub module_id: ModuleId,
    pub module_len: u32,
    pub entry: &'a str,
    pub flags: u8,
    pub sequence: u32,
    pub signature: Option<&'a [u8; SIGNATURE_LEN]>,
    raw_without_sig: &'a [u8],
}

impl<'a> Manifest<'a> {
    /// Parses a manifest from bytes and returns the view plus the remaining module slice.
    pub fn parse(bytes: &'a [u8]) -> Result<(Self, &'a [u8])> {
        if bytes.len() < HEADER_FIXED_V1 {
            return Err(Error::Engine("manifest too small"));
        }
        if &bytes[0..4] != MANIFEST_MAGIC {
            return Err(Error::Engine("manifest magic mismatch"));
        }

        let version = bytes[4];
        match version {
            MANIFEST_VERSION_V1 => Self::parse_v1(bytes),
            MANIFEST_VERSION => Self::parse_v2(bytes),
            _ => Err(Error::Engine("manifest version unsupported")),
        }
    }

    fn parse_v1(bytes: &'a [u8]) -> Result<(Self, &'a [u8])> {
        let module_id = u32::from_le_bytes(bytes[5..9].try_into().unwrap());
        let module_len = u32::from_le_bytes(bytes[9..13].try_into().unwrap());
        let entry_len = bytes[13] as usize;

        let entry_start = HEADER_FIXED_V1;
        let entry_end = entry_start
            .checked_add(entry_len)
            .ok_or(Error::Engine("manifest entry overflow"))?;
        if entry_end > bytes.len() {
            return Err(Error::Engine("manifest entry out of bounds"));
        }
        let entry_bytes = &bytes[entry_start..entry_end];
        let entry = core::str::from_utf8(entry_bytes)
            .map_err(|_| Error::Engine("manifest entry not utf-8"))?;

        let remaining = &bytes[entry_end..];
        let (signature, module_bytes) = if remaining.len() >= SIGNATURE_LEN {
            let (sig, module) = remaining.split_at(SIGNATURE_LEN);
            let sig = sig
                .try_into()
                .map_err(|_| Error::Engine("manifest signature malformed"))?;
            (Some(sig), module)
        } else {
            (None, remaining)
        };

        let raw_without_sig = &bytes[..entry_end];
        Ok((
            Manifest {
                version: MANIFEST_VERSION_V1,
                module_id,
                module_len,
                entry,
                flags: 0,
                sequence: 0,
                signature,
                raw_without_sig,
            },
            module_bytes,
        ))
    }

    fn parse_v2(bytes: &'a [u8]) -> Result<(Self, &'a [u8])> {
        if bytes.len() < HEADER_FIXED_V2 {
            return Err(Error::Engine("manifest too small"));
        }

        let module_id = u32::from_le_bytes(bytes[5..9].try_into().unwrap());
        let module_len = u32::from_le_bytes(bytes[9..13].try_into().unwrap());
        let flags = bytes[13];
        let sequence = u32::from_le_bytes(bytes[14..18].try_into().unwrap());
        let entry_len = bytes[18] as usize;

        let entry_start = HEADER_FIXED_V2;
        let entry_end = entry_start
            .checked_add(entry_len)
            .ok_or(Error::Engine("manifest entry overflow"))?;
        if entry_end > bytes.len() {
            return Err(Error::Engine("manifest entry out of bounds"));
        }
        let entry_bytes = &bytes[entry_start..entry_end];
        let entry = core::str::from_utf8(entry_bytes)
            .map_err(|_| Error::Engine("manifest entry not utf-8"))?;

        let remaining = &bytes[entry_end..];
        let (signature, module_bytes) = if remaining.len() >= SIGNATURE_LEN {
            let (sig, module) = remaining.split_at(SIGNATURE_LEN);
            let sig = sig
                .try_into()
                .map_err(|_| Error::Engine("manifest signature malformed"))?;
            (Some(sig), module)
        } else {
            (None, remaining)
        };

        if (flags & FLAG_REQUIRE_SIGNATURE) != 0 && signature.is_none() {
            return Err(Error::Engine("manifest requires signature"));
        }

        let raw_without_sig = &bytes[..entry_end];
        Ok((
            Manifest {
                version: MANIFEST_VERSION,
                module_id,
                module_len,
                entry,
                flags,
                sequence,
                signature,
                raw_without_sig,
            },
            module_bytes,
        ))
    }

    /// Size of the signing preimage when a signature is present.
    pub fn signing_preimage_len(&self, module_len: usize) -> Option<usize> {
        if self.signature.is_some() {
            Some(self.raw_without_sig.len() + module_len)
        } else {
            None
        }
    }
}

#[cfg(feature = "verify-ed25519")]
/// Verifies the manifest signature against the module bytes using Ed25519.
pub fn verify_ed25519(manifest: &Manifest<'_>, module: &[u8], pubkey: &[u8; 32]) -> Result<()> {
    use ed25519_dalek::{Signature, VerifyingKey};

    let sig_bytes = manifest
        .signature
        .ok_or(Error::Engine("manifest missing signature"))?;

    if manifest.module_len as usize != module.len() {
        return Err(Error::Engine("manifest module_len mismatch"));
    }

    let mut preimage = alloc::vec::Vec::with_capacity(
        manifest
            .signing_preimage_len(module.len())
            .unwrap_or_default(),
    );
    preimage.extend_from_slice(manifest.raw_without_sig);
    preimage.extend_from_slice(module);

    let vk = VerifyingKey::from_bytes(pubkey).map_err(|_| Error::Engine("bad pubkey"))?;
    let sig = Signature::try_from(sig_bytes).map_err(|_| Error::Engine("bad signature bytes"))?;
    vk.verify_strict(&preimage, &sig)
        .map_err(|_| Error::Engine("signature verify failed"))
}

#[cfg(feature = "alloc")]
/// Builds a manifest blob (header + optional signature + module bytes).
pub fn encode(
    module_id: ModuleId,
    entry: &str,
    module: &[u8],
    flags: u8,
    sequence: u32,
    signature: Option<[u8; SIGNATURE_LEN]>,
) -> Result<alloc::vec::Vec<u8>> {
    let header = build_header(module_id, entry, module.len(), flags, sequence)?;

    let mut out = alloc::vec::Vec::with_capacity(
        header.len() + signature.map(|_| SIGNATURE_LEN).unwrap_or(0) + module.len(),
    );
    out.extend_from_slice(&header);
    if let Some(sig) = signature {
        out.extend_from_slice(&sig);
    }
    out.extend_from_slice(module);
    Ok(out)
}

#[cfg(feature = "alloc")]
/// Builds the signing preimage (header + module bytes) for Ed25519 signatures.
pub fn signing_preimage(
    module_id: ModuleId,
    entry: &str,
    module: &[u8],
    flags: u8,
    sequence: u32,
) -> Result<alloc::vec::Vec<u8>> {
    let header = build_header(module_id, entry, module.len(), flags, sequence)?;
    let mut preimage = header;
    preimage.extend_from_slice(module);
    Ok(preimage)
}

#[cfg(feature = "alloc")]
fn build_header(
    module_id: ModuleId,
    entry: &str,
    module_len: usize,
    flags: u8,
    sequence: u32,
) -> Result<alloc::vec::Vec<u8>> {
    if module_len > u32::MAX as usize {
        return Err(Error::Engine("module too large"));
    }

    let entry_bytes = entry.as_bytes();
    if entry_bytes.len() > u8::MAX as usize {
        return Err(Error::Engine("entry name too long"));
    }

    let mut buf = alloc::vec::Vec::with_capacity(HEADER_FIXED_V2 + entry_bytes.len());
    buf.extend_from_slice(MANIFEST_MAGIC);
    buf.push(MANIFEST_VERSION);
    buf.extend_from_slice(&module_id.to_le_bytes());
    buf.extend_from_slice(&(module_len as u32).to_le_bytes());
    buf.push(flags);
    buf.extend_from_slice(&sequence.to_le_bytes());
    buf.push(entry_bytes.len() as u8);
    buf.extend_from_slice(entry_bytes);
    Ok(buf)
}

#[cfg(all(test, feature = "std", feature = "verify-ed25519"))]
mod tests {
    use super::*;
    use ed25519_dalek::Signer;

    #[test]
    fn parses_and_verifies() {
        let signing = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let verifying = signing.verifying_key();

        let module: [u8; 3] = [1, 2, 3];
        let entry = b"main";
        let flags = FLAG_REQUIRE_SIGNATURE | FLAG_ROLLBACK_PROTECTED;
        let sequence = 5u32;

        // Build manifest buffer.
        let mut buf = alloc::vec::Vec::new();
        buf.extend_from_slice(MANIFEST_MAGIC);
        buf.push(MANIFEST_VERSION);
        buf.extend_from_slice(&1u32.to_le_bytes()); // module id
        buf.extend_from_slice(&(module.len() as u32).to_le_bytes());
        buf.push(flags);
        buf.extend_from_slice(&sequence.to_le_bytes());
        buf.push(entry.len() as u8);
        buf.extend_from_slice(entry);

        let mut preimage = buf.clone();
        preimage.extend_from_slice(&module);
        let sig = signing.sign(&preimage);
        buf.extend_from_slice(&sig.to_bytes());
        buf.extend_from_slice(&module);

        let (manifest, module_bytes) = Manifest::parse(&buf).unwrap();
        assert_eq!(manifest.version, MANIFEST_VERSION);
        assert_eq!(manifest.module_id, 1);
        assert_eq!(manifest.entry, "main");
        assert_eq!(manifest.module_len, module.len() as u32);
        assert_eq!(manifest.flags, flags);
        assert_eq!(manifest.sequence, sequence);
        assert!(manifest.signature.is_some());
        assert_eq!(module_bytes, &module);

        verify_ed25519(&manifest, module_bytes, &verifying.to_bytes()).unwrap();
    }

    #[test]
    fn rejects_bad_magic() {
        let bad = [0u8; HEADER_FIXED_V1];
        assert!(Manifest::parse(&bad).is_err());
    }

    #[test]
    fn rejects_missing_sig_when_required() {
        let mut buf = alloc::vec::Vec::new();
        buf.extend_from_slice(MANIFEST_MAGIC);
        buf.push(MANIFEST_VERSION);
        buf.extend_from_slice(&1u32.to_le_bytes()); // module id
        buf.extend_from_slice(&3u32.to_le_bytes()); // module len
        buf.push(FLAG_REQUIRE_SIGNATURE);
        buf.extend_from_slice(&0u32.to_le_bytes()); // sequence
        buf.push(4u8); // entry len
        buf.extend_from_slice(b"main");
        buf.extend_from_slice(&[0u8; 3]); // module bytes (no signature)

        assert!(Manifest::parse(&buf).is_err());
    }
}
