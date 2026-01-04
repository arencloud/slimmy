.PHONY: esp-runtime
.PHONY: test-host test-host-wasm3 all-tests

# Paths to the espup toolchain bits. Adjust if you install a newer toolchain.
ESP_EXPORT ?= $(HOME)/export-esp.sh
ESP_SYSROOT ?= $(HOME)/.rustup/toolchains/esp/xtensa-esp-elf/esp-15.2.0_20250920/xtensa-esp-elf/xtensa-esp-elf
ESP_GCCINC ?= $(HOME)/.rustup/toolchains/esp/xtensa-esp-elf/esp-15.2.0_20250920/xtensa-esp-elf/lib/gcc/xtensa-esp-elf/15.2.0/include
ESP_CLANG ?= $(HOME)/.rustup/toolchains/esp/xtensa-esp32-elf-clang/esp-20.1.1_20250829/esp-clang/bin/clang
ESP_CC ?= $(HOME)/.rustup/toolchains/esp/xtensa-esp-elf/esp-15.2.0_20250920/xtensa-esp-elf/bin/xtensa-esp-elf-gcc

# Bindgen args to avoid host headers (`gnu/stubs-32.h`).
ESP_BINDGEN_ARGS ?= -nostdinc -isystem$(ESP_GCCINC) -isystem$(ESP_SYSROOT)/include -isystem$(ESP_SYSROOT)/sys-include
HOST_CLANG ?= /usr/bin/clang
HOST_SYSROOT ?= /usr

esp-runtime:
	. $(ESP_EXPORT) && \
	ESP_IDF_SYS_ROOT_CRATE=runtime \
	CC=$(ESP_CC) \
	BINDGEN_CLANG_PATH=$(ESP_CLANG) \
	BINDGEN_EXTRA_CLANG_ARGS="$(ESP_BINDGEN_ARGS)" \
	cargo +esp build -Zbuild-std=core,alloc --target xtensa-esp32-espidf \
		-p runtime --no-default-features --features "alloc esp-idf-storage wasm3"

# Host tests (no bindgen):
test-host:
	cargo test

# Host tests with wasm3 + verify-ed25519; force system clang and clear extra bindgen flags.
test-host-wasm3:
	env BINDGEN_EXTRA_CLANG_ARGS="--target=x86_64-unknown-linux-gnu --sysroot=$(HOST_SYSROOT) -isystem/usr/include -isystem/usr/include/x86_64-linux-gnu" \
		CC=$(HOST_CLANG) \
		BINDGEN_CLANG_PATH=$(HOST_CLANG) \
		cargo test --features "wasm3 verify-ed25519"

# Run both host test suites.
all-tests: test-host test-host-wasm3
