//! Storage helpers for pulling modules from flash/ROM or static slices.
//!
//! These are lightweight building blocks meant to be adapted per-target:
//! - `PartitionSliceSource`: map a contiguous region (e.g., ESP-IDF OTA partition, RP2040 XIP).
//! - `IndexedSliceSource`: map multiple modules inside one region using offset/length.
//!
//! The platform-specific glue (NVS/partition reads, STM32 QSPI, etc.) should
//! create a slice over the flash region and feed it into one of these structs.

use crate::{ModuleId, ModuleSource};

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
