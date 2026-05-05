.PHONY: help test boot-test rust-test stage0 stage2 bundle-cli quick-bundle-cli cli clean

STAGE1_WASM ?= target/boot-stage1.wasm
STAGE2_WASM ?= target/boot.wasm
STAGE3_WASM ?= /tmp/twinkle-selfhost/stage3.wasm
TWK_CLI     ?= node tools/twk_cli_sea.cjs

help:
	@printf 'Twinkle development targets:\n'
	@printf '  make test              Run Rust and boot compiler tests\n'
	@printf '  make boot-test         Run boot compiler test suite\n'
	@printf '  make rust-test         Run Rust test suite\n'
	@printf '  make stage0            Build the Rust stage0 compiler\n'
	@printf '  make stage2            Rebuild target/boot.wasm via self-host loop\n'
	@printf '  make bundle-cli        Rebuild stage2 payload and build Node SEA target/twk\n'
	@printf '  make quick-bundle-cli  Build Node SEA target/twk from existing target/boot.wasm\n'
	@printf '  make cli               Alias for bundle-cli\n'

# Fast day-to-day validation for boot compiler changes.
boot-test: target/twk
	target/twk run boot/tests/main.tw

rust-test:
	cargo test

test: rust-test boot-test

# Build the Rust stage0 compiler used to bootstrap the self-hosted compiler.
stage0:
	cargo build --release

# Refresh target/boot.wasm from current boot sources and verify the fixed point.
# Stage0 (Rust) → stage1, stage1 → stage2, stage2 → stage3, then compare stage2 == stage3.
stage2: stage0
	@printf '\n==> Build bridge module for Node runner\n'
	./target/release/twk run boot/tests/gen_bridge_wasm.tw
	@printf '\n==> Build stage1 compiler with stage0 -> $(STAGE1_WASM)\n'
	./target/release/twk build boot/main.tw -o $(STAGE1_WASM)
	@printf '\n==> Self-hosted check via stage1\n'
	BOOT_WASM=$(STAGE1_WASM) $(TWK_CLI) check boot/main.tw
	@printf '\n==> Build stage2 compiler with stage1 -> $(STAGE2_WASM)\n'
	BOOT_WASM=$(STAGE1_WASM) $(TWK_CLI) build boot/main.tw -o $(STAGE2_WASM)
	@printf '\n==> Build stage3 compiler with stage2 -> $(STAGE3_WASM)\n'
	@mkdir -p $(dir $(STAGE3_WASM))
	BOOT_WASM=$(STAGE2_WASM) $(TWK_CLI) build boot/main.tw -o $(STAGE3_WASM)
	@printf '\n==> Compare stage2 and stage3 WASM\n'
	@cmp -s $(STAGE2_WASM) $(STAGE3_WASM) \
		&& printf 'Fixed point reached: $(STAGE2_WASM) == $(STAGE3_WASM)\n' \
		|| { printf 'error: fixed point mismatch; compare files: $(STAGE2_WASM) $(STAGE3_WASM)\n' >&2; exit 1; }
	@printf '\nSelf-host loop completed successfully.\n'

# Build the Node SEA standalone CLI from target/boot.wasm.
target/twk: $(STAGE2_WASM)
	tools/build_node_sea_cli.sh

# Full standalone CLI rebuild: stage2 payload + Node SEA.
bundle-cli: stage2 target/twk

cli: bundle-cli

# Rebuild the standalone CLI from the existing target/boot.wasm without rebuilding
# the self-hosted payload. This is only correct when target/boot.wasm is already fresh.
quick-bundle-cli:
	tools/build_node_sea_cli.sh

clean:
	cargo clean
	rm -f target/boot.wasm target/boot-stage1.wasm target/twk
