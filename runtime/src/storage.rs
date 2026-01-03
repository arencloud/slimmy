//! Storage helpers for pulling modules from flash/ROM or static slices.
//!
//! These are lightweight building blocks meant to be adapted per-target:
//! - `PartitionSliceSource`: map a contiguous region (e.g., ESP-IDF OTA partition, RP2040 XIP).
//! - `IndexedSliceSource`: map multiple modules inside one region using offset/length.
//! - `FlashBufferedSource`: simple flash-backed store that copies into RAM when fetched.
//!
//! The platform-specific glue (NVS/partition reads, STM32 QSPI, etc.) should
//! create a slice over the flash region and feed it into one of these structs.

use crate::{Error, ModuleId, ModuleSource, Result};
#[cfg(feature = "std")]
use std::fs::{OpenOptions};
#[cfg(feature = "std")]
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(feature = "std")]
use std::path::PathBuf;

/// Treats a single contiguous slice as one module with a fixed id.
pub struct PartitionSliceSource<'a> {
    region: &'a [u8],
    id: ModuleId,
}

impl<'a> PartitionSliceSource<'a> {
    /// Creates a new source backed by a contiguous flash/ROM region.
    pub const fn new(region: &'a [u8], id: ModuleId) -> Self {
        Self { region, id }
    }
}

impl<'a> ModuleSource for PartitionSliceSource<'a> {
    fn fetch(&self, id: ModuleId) -> Option<&[u8]> {
        if id == self.id {
            Some(self.region)
        } else {
            None
        }
    }
}

/// Maps multiple modules within a single backing slice.
///
/// Offsets and lengths should respect the erase/program boundaries of the target
/// flash device. This keeps storage policy out of the core runtime.
pub struct IndexedSliceSource<'a> {
    region: &'a [u8],
    entries: &'a [IndexEntry],
}

/// Simple offset/len entry for modules in a region.
#[derive(Clone, Copy)]
pub struct IndexEntry {
    pub id: ModuleId,
    pub offset: usize,
    pub len: usize,
}

impl<'a> IndexedSliceSource<'a> {
    /// Creates an indexed source over a shared backing slice.
    pub const fn new(region: &'a [u8], entries: &'a [IndexEntry]) -> Self {
        Self { region, entries }
    }
}

impl<'a> ModuleSource for IndexedSliceSource<'a> {
    fn fetch(&self, id: ModuleId) -> Option<&[u8]> {
        let entry = self.entries.iter().find(|e| e.id == id)?;
        let end = entry.offset.checked_add(entry.len)?;
        self.region.get(entry.offset..end)
    }
}

/// ESP-IDF note:
/// Use `unsafe { core::slice::from_raw_parts(base_ptr, len) }` where `base_ptr`
/// points at an OTA/NVS partition mapped into the address space, then wrap it
/// with `PartitionSliceSource` or `IndexedSliceSource`.
///
/// STM32 note:
/// For internal flash or QSPI-mapped flash, expose a `[u8]` view over the region
/// and feed it into the same helpers. If flash is not memory-mapped, use an
/// in-RAM cache that you keep alive for the lifetime of `IndexedSliceSource`.
#[allow(dead_code)]
fn platform_notes() {}

/// ESP-IDF flash-backed implementation using esp-idf-sys (interpreter targets).
#[cfg(all(feature = "esp-idf-storage", target_os = "espidf"))]
pub mod esp_idf {
    use super::*;
    use alloc::ffi::CString;

    pub struct PartitionFlash {
        part: *const esp_idf_sys::esp_partition_t,
    }

    unsafe impl Send for PartitionFlash {}
    unsafe impl Sync for PartitionFlash {}

    impl PartitionFlash {
        /// Creates from an existing partition pointer.
        pub unsafe fn from_raw(part: *const esp_idf_sys::esp_partition_t) -> Result<Self> {
            if part.is_null() {
                return Err(Error::Engine("null partition"));
            }
            Ok(Self { part })
        }

        /// Finds the first data partition matching label.
        pub fn from_label(label: &str) -> Result<Self> {
            let c_label = CString::new(label).map_err(|_| Error::Engine("bad label"))?;
            let part = unsafe {
                esp_idf_sys::esp_partition_find_first(
                    esp_idf_sys::esp_partition_type_t_ESP_PARTITION_TYPE_DATA,
                    esp_idf_sys::esp_partition_subtype_t_ESP_PARTITION_SUBTYPE_ANY,
                    c_label.as_ptr(),
                )
            };
            if part.is_null() {
                return Err(Error::Engine("partition not found"));
            }
            Ok(Self { part })
        }
    }

    impl FlashIo for PartitionFlash {
        fn erase_write(&mut self, offset: usize, data: &[u8]) -> Result<()> {
            let res = unsafe {
                esp_idf_sys::esp_partition_erase_range(self.part, offset as u32, data.len() as u32)
            };
            if res != esp_idf_sys::esp_err_t_ESP_OK {
                return Err(Error::Engine("partition erase failed"));
            }
            let res = unsafe {
                esp_idf_sys::esp_partition_write(
                    self.part,
                    offset as u32,
                    data.as_ptr() as *const _,
                    data.len(),
                )
            };
            if res != esp_idf_sys::esp_err_t_ESP_OK {
                return Err(Error::Engine("partition write failed"));
            }
            Ok(())
        }

        fn read(&self, offset: usize, buf: &mut [u8]) -> Result<()> {
            let res = unsafe {
                esp_idf_sys::esp_partition_read(
                    self.part,
                    offset as u32,
                    buf.as_mut_ptr() as *mut _,
                    buf.len(),
                )
            };
            if res != esp_idf_sys::esp_err_t_ESP_OK {
                return Err(Error::Engine("partition read failed"));
            }
            Ok(())
        }

        fn capacity(&self) -> usize {
            unsafe { (*self.part).size as usize }
        }
    }

    /// Convenience alias for a buffered store over an ESP-IDF partition.
    pub type PartitionBufferedStore = FlashBufferedSource<PartitionFlash>;
}

/// STM32/QSPI flash-backed integration helper using function pointers.
#[cfg(feature = "stm32-storage")]
pub mod stm32 {
    use super::*;

    pub struct HalFlash {
        erase_write: fn(usize, &[u8]) -> Result<()>,
        read_fn: fn(usize, &mut [u8]) -> Result<()>,
        capacity: usize,
    }

    impl HalFlash {
        pub const fn new(
            erase_write: fn(usize, &[u8]) -> Result<()>,
            read_fn: fn(usize, &mut [u8]) -> Result<()>,
            capacity: usize,
        ) -> Self {
            Self {
                erase_write,
                read_fn,
                capacity,
            }
        }
    }

    impl FlashIo for HalFlash {
        fn erase_write(&mut self, offset: usize, data: &[u8]) -> Result<()> {
            (self.erase_write)(offset, data)
        }

        fn read(&self, offset: usize, buf: &mut [u8]) -> Result<()> {
            (self.read_fn)(offset, buf)
        }

        fn capacity(&self) -> usize {
            self.capacity
        }
    }

    /// Convenience aliases for buffered/on-demand stores backed by HAL flash fns.
    pub type HalBufferedStore = FlashBufferedSource<HalFlash>;
    pub type HalOnDemandStore = FlashOnDemandSource<HalFlash>;
}

/// File-backed flash emulator for host testing. Not for production.
#[cfg(feature = "std")]
pub struct FileFlash {
    path: PathBuf,
    capacity: usize,
}

#[cfg(feature = "std")]
impl FileFlash {
    pub fn new(path: PathBuf, capacity: usize) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .map_err(|_| Error::Engine("open flash file"))?;
        file.set_len(capacity as u64)
            .map_err(|_| Error::Engine("size flash file"))?;
        Ok(Self { path, capacity })
    }

    fn with_file<F>(&self, mut f: F) -> Result<()>
    where
        F: FnMut(&mut std::fs::File) -> Result<()>,
    {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .map_err(|_| Error::Engine("open flash file"))?;
        f(&mut file)
    }
}

#[cfg(feature = "std")]
impl FlashIo for FileFlash {
    fn erase_write(&mut self, offset: usize, data: &[u8]) -> Result<()> {
        let end = offset + data.len();
        if end > self.capacity {
            return Err(Error::Engine("write out of bounds"));
        }

        self.with_file(|f| {
            f.seek(SeekFrom::Start(offset as u64))
                .map_err(|_| Error::Engine("seek flash file"))?;
            f.write_all(data)
                .map_err(|_| Error::Engine("write flash file"))?;
            Ok(())
        })
    }

    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<()> {
        let end = offset + buf.len();
        if end > self.capacity {
            return Err(Error::Engine("read out of bounds"));
        }

        let mut file = OpenOptions::new()
            .read(true)
            .open(&self.path)
            .map_err(|_| Error::Engine("open flash file"))?;
        file.seek(SeekFrom::Start(offset as u64))
            .map_err(|_| Error::Engine("seek flash file"))?;
        file.read_exact(buf)
            .map_err(|_| Error::Engine("read flash file"))
    }

    fn capacity(&self) -> usize {
        self.capacity
    }
}

/// Generic flash I/O abstraction to back platform-specific ModuleSource implementations.
#[cfg(feature = "alloc")]
pub trait FlashIo {
    /// Erases and writes data at the given offset. Offsets must be erase-block aligned per chip rules.
    fn erase_write(&mut self, offset: usize, data: &[u8]) -> Result<()>;
    /// Reads a range into the provided buffer.
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<()>;
    /// Returns total capacity in bytes.
    fn capacity(&self) -> usize;
}

/// Simple flash-backed source that copies a single module into RAM when fetched.
#[cfg(feature = "alloc")]
pub struct FlashBufferedSource<IO: FlashIo> {
    io: IO,
    base_offset: usize,
    len: usize,
    module_id: ModuleId,
    cache: alloc::vec::Vec<u8>,
}

#[cfg(feature = "alloc")]
impl<IO: FlashIo> FlashBufferedSource<IO> {
    pub fn new(io: IO, base_offset: usize, len: usize, module_id: ModuleId) -> Self {
        Self {
            io,
            base_offset,
            len,
            module_id,
            cache: alloc::vec::Vec::new(),
        }
    }

    /// Writes a module into flash, truncating/padding to len.
    pub fn write_module(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.len {
            return Err(Error::Engine("flash slot too small"));
        }
        self.io.erase_write(self.base_offset, bytes)?;
        Ok(())
    }
}

#[cfg(feature = "alloc")]
impl<IO: FlashIo> ModuleSource for FlashBufferedSource<IO> {
    fn fetch(&self, id: ModuleId) -> Option<&[u8]> {
        if id != self.module_id {
            return None;
        }
        if self.cache.is_empty() {
            None
        } else {
            Some(self.cache.as_slice())
        }
    }
}

#[cfg(feature = "alloc")]
impl<IO: FlashIo> FlashBufferedSource<IO> {
    /// Loads from flash into the cache buffer and returns it.
    pub fn fetch_into_cache(&mut self) -> Result<&[u8]> {
        self.cache.resize(self.len, 0);
        self.io
            .read(self.base_offset, &mut self.cache)
            .map_err(|_| Error::Engine("flash read failed"))?;
        Ok(self.cache.as_slice())
    }

    /// Returns cached slice if present, otherwise loads from flash.
    pub fn fetch_or_load(&mut self) -> Result<&[u8]> {
        if self.cache.is_empty() {
            self.fetch_into_cache()
        } else {
            Ok(self.cache.as_slice())
        }
    }
}

/// On-demand flash source that reads directly from flash each fetch (no cache).
#[cfg(feature = "alloc")]
pub struct FlashOnDemandSource<IO: FlashIo> {
    io: IO,
    base_offset: usize,
    len: usize,
    module_id: ModuleId,
    scratch: alloc::vec::Vec<u8>,
}

#[cfg(feature = "alloc")]
impl<IO: FlashIo> FlashOnDemandSource<IO> {
    pub fn new(io: IO, base_offset: usize, len: usize, module_id: ModuleId) -> Self {
        Self {
            io,
            base_offset,
            len,
            module_id,
            scratch: alloc::vec::Vec::new(),
        }
    }
}

#[cfg(feature = "alloc")]
impl<IO: FlashIo> ModuleSource for FlashOnDemandSource<IO> {
    fn fetch(&self, id: ModuleId) -> Option<&[u8]> {
        if id != self.module_id {
            None
        } else {
            // fetch() cannot mutate scratch; use fetch_into to populate before calling.
            if self.scratch.is_empty() {
                None
            } else {
                Some(self.scratch.as_slice())
            }
        }
    }
}

#[cfg(feature = "alloc")]
impl<IO: FlashIo> FlashOnDemandSource<IO> {
    /// Reads the module into the provided buffer; buffer length must match `len`.
    pub fn read_into<'b>(&self, buf: &'b mut [u8]) -> Result<&'b [u8]> {
        if buf.len() != self.len {
            return Err(Error::Engine("buffer len mismatch"));
        }
        self.io
            .read(self.base_offset, buf)
            .map_err(|_| Error::Engine("flash read failed"))?;
        Ok(buf)
    }

    /// Reads the module into the internal scratch buffer and returns it.
    pub fn fetch_into_scratch(&mut self) -> Result<&[u8]> {
        self.scratch.resize(self.len, 0);
        self.io
            .read(self.base_offset, self.scratch.as_mut_slice())
            .map_err(|_| Error::Engine("flash read failed"))?;
        Ok(self.scratch.as_slice())
    }
}

/// In-memory flash implementation (useful for tests or RAM-only targets).
#[cfg(feature = "alloc")]
pub struct MemoryFlash {
    storage: alloc::vec::Vec<u8>,
}

#[cfg(feature = "alloc")]
impl MemoryFlash {
    pub fn new(size: usize) -> Self {
        Self {
            storage: alloc::vec![0xFF; size],
        }
    }
}

#[cfg(feature = "alloc")]
impl FlashIo for MemoryFlash {
    fn erase_write(&mut self, offset: usize, data: &[u8]) -> Result<()> {
        let end = offset + data.len();
        if end > self.storage.len() {
            return Err(Error::Engine("write out of bounds"));
        }
        self.storage[offset..end].copy_from_slice(data);
        Ok(())
    }

    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<()> {
        let end = offset + buf.len();
        if end > self.storage.len() {
            return Err(Error::Engine("read out of bounds"));
        }
        buf.copy_from_slice(&self.storage[offset..end]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.storage.len()
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    #[cfg(feature = "std")]
    use std::env;
    #[cfg(feature = "std")]
    use std::fs;

    struct MockFlash {
        storage: Vec<u8>,
    }

    impl MockFlash {
        fn new(size: usize) -> Self {
            Self {
                storage: vec![0xFF; size],
            }
        }
    }

    impl FlashIo for MockFlash {
        fn erase_write(&mut self, offset: usize, data: &[u8]) -> Result<()> {
            let end = offset + data.len();
            if end > self.storage.len() {
                return Err(Error::Engine("write out of bounds"));
            }
            self.storage[offset..end].copy_from_slice(data);
            Ok(())
        }

        fn read(&self, offset: usize, buf: &mut [u8]) -> Result<()> {
            let end = offset + buf.len();
            if end > self.storage.len() {
                return Err(Error::Engine("read out of bounds"));
            }
            buf.copy_from_slice(&self.storage[offset..end]);
            Ok(())
        }

        fn capacity(&self) -> usize {
            self.storage.len()
        }
    }

    #[test]
    fn flash_buffered_source_loads_from_flash() {
        let flash = MockFlash::new(64);
        let mut source = FlashBufferedSource::new(flash, 0, 8, 7);

        source.write_module(&[1, 2, 3, 4]).unwrap();
        let bytes = source.fetch_or_load().unwrap();
        assert_eq!(bytes[..4], [1, 2, 3, 4]);
    }

    #[test]
    fn flash_write_rejects_large_module() {
        let flash = MockFlash::new(8);
        let mut source = FlashBufferedSource::new(flash, 0, 4, 1);
        assert!(source.write_module(&[0u8; 8]).is_err());
    }

    #[cfg(feature = "std")]
    #[test]
    fn file_flash_io_roundtrip() {
        let tmp = env::temp_dir().join("slimmy_flash.bin");
        let _ = fs::remove_file(&tmp);

        let mut flash = FileFlash::new(tmp.clone(), 16).unwrap();
        flash.erase_write(0, &[9, 8, 7, 6]).unwrap();

        let mut buf = [0u8; 4];
        flash.read(0, &mut buf).unwrap();
        assert_eq!(buf, [9, 8, 7, 6]);

        let _ = fs::remove_file(tmp);
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn flash_on_demand_reads_from_flash() {
        let mut flash = MemoryFlash::new(8);
        flash.erase_write(0, &[5, 6, 7, 8]).unwrap();

        let mut source = FlashOnDemandSource::new(flash, 0, 4, 3);
        let bytes = source.fetch_into_scratch().unwrap();
        assert_eq!(bytes, &[5, 6, 7, 8]);
    }
}
