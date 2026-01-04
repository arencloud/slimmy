.PHONY: esp-runtime

# Paths to the espup toolchain bits. Adjust if you install a newer toolchain.
ESP_EXPORT ?= $(HOME)/export-esp.sh
ESP_SYSROOT ?= $(HOME)/.rustup/toolchains/esp/xtensa-esp-elf/esp-15.2.0_20250920/xtensa-esp-elf/xtensa-esp-elf
ESP_GCCINC ?= $(HOME)/.rustup/toolchains/esp/xtensa-esp-elf/esp-15.2.0_20250920/xtensa-esp-elf/lib/gcc/xtensa-esp-elf/15.2.0/include
ESP_CLANG ?= $(HOME)/.rustup/toolchains/esp/xtensa-esp32-elf-clang/esp-20.1.1_20250829/esp-clang/bin/clang

# Bindgen args to avoid host headers (`gnu/stubs-32.h`).
ESP_BINDGEN_ARGS ?= -nostdinc -isystem$(ESP_GCCINC) -isystem$(ESP_SYSROOT)/include -isystem$(ESP_SYSROOT)/sys-include

esp-runtime:
	. $(ESP_EXPORT) && \
	BINDGEN_CLANG_PATH=$(ESP_CLANG) \
	BINDGEN_EXTRA_CLANG_ARGS="$(ESP_BINDGEN_ARGS)" \
	cargo +esp build -Zbuild-std=core,alloc --target xtensa-esp32-espidf \
		-p runtime --no-default-features --features "alloc esp-idf-storage wasm3"
