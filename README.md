# slimmy

Tiny OTA-deliverable WebAssembly runner for embedded targets (ESP32, STM32, nRF52, RP2040) and ARM/x86 edge devices. Keep firmware static; ship new logic as 20–200 KB `.wasm` blobs.

**Author:** Eduard Gevorkyan `<egevorky@arencloud.com>`  
**License:** Apache-2.0

## What’s inside
- `runtime/` – no_std core traits (`Engine`, `ModuleSource`), `Runtime` orchestrator, `MemoryStore`, `CachedEngine`, storage helpers.
- `runtime::manifest` – header (`SMNY` v2: flags + sequence) + optional Ed25519 verify (`verify-ed25519` feature); encode + signing preimage helpers.
- `runtime::engines::wasm3` – minimal wasm3 interpreter backend (`engine-wasm3` feature).
- `runtime::engines::wamr` – stub feature (`engine-wamr`) for future integration.
- `runtime::engines::wasmtime_lite` – host-only wasmtime backend for testing (`engine-wasmtime-lite`), not for MCU targets.
- `runtime::storage` – memory-mapped helpers, `FlashIo` trait, `FlashBufferedSource`, `FlashOnDemandSource`, `MemoryFlash`/`FileFlash` for host tests, ESP-IDF (`esp-idf-storage`) and STM32 (`stm32-storage`) adapters + builder helpers.
- `host-demo/` – CLI harness; can run no-op engine or wasm3 (`--features wasm3`).
- `guest-wasm/` – tiniest example module (`main()` no args/returns) built for `wasm32-unknown-unknown`.
- `packer/` – host-side packer to wrap `.wasm` into manifest, optionally sign with Ed25519.

## Quick start
- Build sample wasm: `cargo build -p guest-wasm --target wasm32-unknown-unknown --release`
- Run host demo (no-op): `cargo run -p host-demo -- guest-wasm/target/wasm32-unknown-unknown/release/guest_wasm.wasm main`
- Run host demo with wasm3: `cargo run -p host-demo --features wasm3 -- guest-wasm/target/wasm32-unknown-unknown/release/guest_wasm.wasm main`
  - Requires `clang`; uses vendored `wasm3-sys` with build-bindgen.
- Run host demo with wasmtime (host only): `cargo run -p host-demo --features wasmtime-lite -- guest-wasm/target/wasm32-unknown-unknown/release/guest_wasm.wasm main`
- Pack manifest (unsigned): `cargo run -p packer -- guest-wasm/target/wasm32-unknown-unknown/release/guest_wasm.wasm -o module.smny`
- Pack manifest (signed + flags): `cargo run -p packer -- --module-id 1 --entry main --sequence 7 --require-signature --sign-key-hex <32-byte-hex> guest-wasm/target/wasm32-unknown-unknown/release/guest_wasm.wasm -o module.smny.sig`
- Run tests (no-op path): `cargo test`
- Run tests with wasm3 + verify-ed25519: `cargo test --features "wasm3 verify-ed25519"`

## Runtime design
- `Engine` abstraction: swap wasm3/WAMR/wasmtime-lite; no_std-friendly; errors stay tiny (`&'static str`).
- `ModuleSource`: pluggable storage (flash, NVS, QSPI, RAM).
- `Runtime`: load + invoke orchestration only.
- `MemoryStore` (alloc): RAM-backed store; `CachedEngine` reuses loaded handles.
- Storage helpers: `PartitionSliceSource`, `IndexedSliceSource` map memory-mapped flash regions (ESP-IDF OTA/NVS, RP2040 XIP, STM32 QSPI) into `ModuleSource`.
- Manifest: magic `SMNY`, version 2 (flags + sequence + entry), optional 64-byte Ed25519 signature over header||module. Flags: bit0 require signature, bit1 rollback-protected (use sequence).

## Target notes
- ESP32 (esp-idf): wasm3 (`m3_config_platform_esp32`) or WAMR interpreter; modules in NVS/flash; use `esp-idf-svc` std shim. Storage helpers include `buffered_store_ota1` / `on_demand_store_ota1` (feature `esp-idf-storage`) targeting `ota_1` by default.
- STM32 / nRF52: bare-metal `no_std + alloc`; interpreter mode; flash-backed `ModuleSource` with erase-aligned buffers. Use `HalFlash::new(erase_write, read, capacity, erase_block)` to enforce sector alignment (`erase_block=0` to skip check) and the builders `buffered_store_from_hal` / `on_demand_store_from_hal`.
- RP2040: wasm3 fits; modules in XIP flash or littlefs; OTA via UF2 carrying only `.wasm`.
- Linux/x86_64/aarch64: wasmtime-lite or wasm3 for integration tests.

## Roadmap
1) Wire real WAMR engine with size-tuned config (replace stub).
2) Harden wasmtime-lite backend if kept (host-only) or replace with wasmtime-lite embedding.
3) Flash-backed `ModuleSource` implementations (ESP-IDF partitions via `esp-idf-storage`, STM32 HAL erase/write-safe paths via `stm32-storage`).
4) Hardened manifest toolchain: versioned manifest, signature policy, rollback guard.
5) CI: extend to platform cross-checks (esp-idf, STM32) once toolchains are available.
