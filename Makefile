.PHONY: help test boot-test rust-test stage0 stage2 bundle-cli quick-bundle-cli cli playground playground-dev clean

STAGE1_WASM ?= target/boot-stage1.wasm
STAGE2_WASM ?= target/boot.wasm
STAGE3_WASM ?= /tmp/twinkle-selfhost/stage3.wasm
STAGE4_WASM ?= /tmp/twinkle-selfhost/stage4.wasm
TWK_CLI     ?= node target/twk_cli_sea.cjs
SEA_NODE_VERSION ?= 26
SEA_NODE_BIN ?=

# Source file sets — used for dependency tracking.
RUST_SRCS := $(shell find src -name '*.rs') Cargo.toml Cargo.lock
BOOT_SRCS := $(shell find boot -name '*.tw' -not -path 'boot/tests/*' -not -path 'boot/tmp/*' -not -path 'boot/repros/*')
CORE_LIB_SRCS := $(shell find prelude stdlib -name '*.tw')

help:
	@printf 'Twinkle development targets:\n'
	@printf '  make test              Run Rust and boot compiler tests\n'
	@printf '  make boot-test         Run boot compiler test suite\n'
	@printf '  make rust-test         Run Rust test suite\n'
	@printf '  make stage0            Build the Rust stage0 compiler\n'
	@printf '  make stage2            Rebuild target/boot.wasm via self-host loop\n'
	@printf '  make bundle-cli        Rebuild stage2 payload and build Node SEA target/twk\n'
	@printf '  make cli               Alias for bundle-cli\n'
	@printf '  make playground        Build playground (all deps + vite build)\n'
	@printf '  make playground-dev    Start playground dev server (all deps + vite dev)\n'

# Fast day-to-day validation for boot compiler changes.
boot-test: target/twk
	target/twk run boot/tests/main.tw

rust-test:
	cargo test --release

test: rust-test boot-test

# Build the Rust stage0 compiler used to bootstrap the self-hosted compiler.
# File target so downstream rules rebuild only when Rust sources change.
stage0: target/release/twk

target/release/twk: $(RUST_SRCS)
	cargo build --release

# Refresh target/boot.wasm from current boot sources and verify the fixed point.
# Stage0 (Rust) → stage1, stage1 → stage2, stage2 → stage3, stage3 → stage4.
# Compare stage3 == stage4 (both built by boot compilers, avoiding Rust/boot
# encoder divergence in the stage2 vs stage3 comparison).
stage2: $(STAGE2_WASM)

boot/lib/module/core_lib.tw: $(CORE_LIB_SRCS) tools/generate_core_lib.tw target/release/twk
	./target/release/twk run tools/generate_core_lib.tw

# Bundle the shared JS runtime into the CJS entry used by the Node runner.
target/twk_cli_sea.cjs: tools/js_runtime/runtime.mjs tools/js_runtime/sea_main.mjs
	@mkdir -p target
	npx --yes esbuild tools/js_runtime/sea_main.mjs --bundle --platform=node --format=cjs --outfile=target/twk_cli_sea.cjs

$(STAGE2_WASM): $(BOOT_SRCS) $(CORE_LIB_SRCS) boot/lib/module/core_lib.tw target/release/twk target/twk_cli_sea.cjs
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
	@printf '\n==> Adopt stage3 as stage2 (converge to boot-compiled baseline)\n'
	@cp $(STAGE3_WASM) $(STAGE2_WASM)
	@printf '\n==> Build stage4 compiler with stage3 -> $(STAGE4_WASM)\n'
	BOOT_WASM=$(STAGE2_WASM) $(TWK_CLI) build boot/main.tw -o $(STAGE4_WASM)
	@printf '\n==> Compare stage3 and stage4 WASM\n'
	@cmp -s $(STAGE2_WASM) $(STAGE4_WASM) \
		&& printf 'Fixed point reached: stage3 == stage4\n' \
		|| { printf 'error: fixed point mismatch; compare files: $(STAGE2_WASM) $(STAGE4_WASM)\n' >&2; exit 1; }
	@printf '\nSelf-host loop completed successfully.\n'

# Build the Node SEA standalone CLI from target/boot.wasm.
target/twk: $(STAGE2_WASM) tools/build_node_sea_cli.sh tools/find_node_sea_bin.sh tools/js_runtime/runtime.mjs tools/js_runtime/sea_main.mjs
	NODE_BIN="$$(tools/find_node_sea_bin.sh "$(SEA_NODE_VERSION)" "$(SEA_NODE_BIN)")" tools/build_node_sea_cli.sh

# Full standalone CLI rebuild: stage2 payload + Node SEA.
bundle-cli: stage2 target/twk

cli: bundle-cli

# Rebuild the standalone CLI from the existing target/boot.wasm without rebuilding
# the self-hosted payload. This is only correct when target/boot.wasm is already fresh.
quick-bundle-cli:
	NODE_BIN="$$(tools/find_node_sea_bin.sh "$(SEA_NODE_VERSION)" "$(SEA_NODE_BIN)")" tools/build_node_sea_cli.sh

# ---------------------------------------------------------------------------
# Playground
# ---------------------------------------------------------------------------

# Bridge module used by the JS runners
tools/bridge.wasm: boot/tests/gen_bridge_wasm.tw target/release/twk
	./target/release/twk run boot/tests/gen_bridge_wasm.tw

# Tree-sitter grammar wasm (rebuild when grammar.js changes)
tree-sitter-twinkle/tree-sitter-twinkle.wasm: tree-sitter-twinkle/grammar.js
	cd tree-sitter-twinkle && npx tree-sitter generate && npx tree-sitter build --wasm

# Ensure playground npm deps are installed
playground/node_modules: playground/package.json playground/package-lock.json
	cd playground && npm ci && touch node_modules

# Copy all artifacts into playground/public/, then run vite build
playground: $(STAGE2_WASM) tools/bridge.wasm tree-sitter-twinkle/tree-sitter-twinkle.wasm playground/node_modules
	cd playground && node scripts/copy-assets.mjs && npx vite build

# Copy artifacts and start the vite dev server
playground-dev: $(STAGE2_WASM) tools/bridge.wasm tree-sitter-twinkle/tree-sitter-twinkle.wasm playground/node_modules
	cd playground && node scripts/copy-assets.mjs && npx vite

clean:
	cargo clean
	rm -f target/boot.wasm target/boot-stage1.wasm target/twk target/twk_cli_sea.cjs
