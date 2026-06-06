.PHONY: help test boot-test rust-test stage0 stage2 bundle-cli quick-bundle-cli cli playground playground-dev fmt bench bench-guard clean npm-pack npm-publish npm-test

STAGE1_WASM ?= target/boot-stage1.wasm
STAGE2_WASM ?= target/boot.wasm
STAGE3_WASM ?= /tmp/twinkle-selfhost/stage3.wasm
STAGE4_WASM ?= /tmp/twinkle-selfhost/stage4.wasm
DENO_BIN    ?= deno
TWK_CLI     ?= $(DENO_BIN) run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs

# Source file sets — used for dependency tracking.
RUST_SRCS := $(shell find src -name '*.rs') Cargo.toml Cargo.lock
BOOT_SRCS := $(shell find boot -name '*.tw' -not -path 'boot/tests/*' -not -path 'boot/tmp/*' -not -path 'boot/repros/*' -not -path 'boot/bench/*' -not -path 'boot/prelude/*' -not -path 'boot/stdlib/*')
CORE_LIB_SRCS := $(shell find boot/prelude boot/stdlib -name '*.tw')

help:
	@printf 'Twinkle development targets:\n'
	@printf '  make test              Run Rust and boot compiler tests\n'
	@printf '  make boot-test         Run boot compiler test suite\n'
	@printf '  make rust-test         Run Rust test suite\n'
	@printf '  make stage0            Build the Rust stage0 compiler\n'
	@printf '  make stage2            Rebuild target/boot.wasm via self-host loop\n'
	@printf '  make bundle-cli        Rebuild stage2 payload and build Deno target/twk\n'
	@printf '  make cli               Alias for bundle-cli\n'
	@printf '  make fmt               Format boot compiler .tw source files\n'
	@printf '  make bench             Run the Vector benchmark suite (boot/bench/)\n'
	@printf '  make bench-guard       Check vector scaling/bulk-copy guards\n'
	@printf '  make playground        Build playground from published packages (no compiler build)\n'
	@printf '  make playground-dev    Dev server against the in-repo compiler (TWINKLE_LOCAL)\n'

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

boot/lib/module/core_lib.tw: $(CORE_LIB_SRCS) tools/generate_core_lib.py
	python3 tools/generate_core_lib.py
	@if [ -x target/twk ]; then \
		target/twk fmt boot/lib/module/core_lib.tw; \
	else \
		printf 'target/twk not available; skipping core_lib formatting\n'; \
	fi

$(STAGE2_WASM): $(BOOT_SRCS) $(CORE_LIB_SRCS) boot/lib/module/core_lib.tw target/release/twk tools/js_runtime/runtime.mjs tools/js_runtime/deno_main.mjs tools/js_runtime/node_host.mjs
	@printf '\n==> Build stage1 compiler with stage0 -> $(STAGE1_WASM)\n'
	./target/release/twk build boot/main.tw -o $(STAGE1_WASM)
	@printf '\n==> Build bridge module via stage1\n'
	BOOT_WASM=$(STAGE1_WASM) $(TWK_CLI) run boot/tests/gen_bridge_wasm.tw
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

# Build the Deno standalone CLI from target/boot.wasm.
target/twk: $(STAGE2_WASM) tools/build_deno_cli.sh tools/js_runtime/runtime.mjs tools/js_runtime/deno_main.mjs tools/js_runtime/node_host.mjs
	DENO_BIN="$(DENO_BIN)" tools/build_deno_cli.sh

# Full standalone CLI rebuild: stage2 payload + Deno compile.
bundle-cli: stage2 target/twk

cli: bundle-cli

# Rebuild the standalone CLI from the existing target/boot.wasm without rebuilding
# the self-hosted payload. This is only correct when target/boot.wasm is already fresh.
quick-bundle-cli:
	DENO_BIN="$(DENO_BIN)" tools/build_deno_cli.sh

# ---------------------------------------------------------------------------
# Playground
# ---------------------------------------------------------------------------
#
# The playground is a plain Vite app that consumes the published packages
# (@twinkle-lang/twinkle for the compiler runtime + boot.wasm/bridge.wasm, and
# tree-sitter-twinkle for the grammar wasm + highlight query).
#
# `make playground` builds from those published packages (just npm + vite, no
# Rust/Deno/self-host) — this is what the GitHub Pages deploy runs, so the live
# site tracks the last published @twinkle-lang/twinkle and the deploy stays cheap.
#
# `make playground-dev` runs the dev server against the in-repo compiler: it
# builds target/boot.wasm and sets TWINKLE_LOCAL=1 so Vite aliases the package
# specifiers to current in-repo artifacts, letting you test unreleased changes.

# Ensure playground npm deps are installed
playground/node_modules: playground/package.json playground/package-lock.json
	cd playground && npm ci && touch node_modules

# Build from the published packages (cheap; no compiler build).
playground: playground/node_modules
	cd playground && npx vite build

# Tree-sitter grammar wasm (rebuild when grammar.js changes; needs Docker)
tree-sitter-twinkle/tree-sitter-twinkle.wasm: tree-sitter-twinkle/grammar.js
	cd tree-sitter-twinkle && npx tree-sitter generate && npx tree-sitter build --wasm

# Dev server against the in-repo compiler (TWINKLE_LOCAL aliases to in-repo
# artifacts), so unreleased compiler/runtime changes show up in the playground.
# web.mjs self-loads wasm via `new URL('./boot.wasm', import.meta.url)`, so stage
# the in-repo wasm next to it (the published package ships them flattened).
playground-dev: $(STAGE2_WASM) tools/bridge.wasm tools/js_runtime/runtime.mjs tools/js_runtime/web.mjs tree-sitter-twinkle/tree-sitter-twinkle.wasm playground/node_modules
	cp $(STAGE2_WASM) tools/js_runtime/boot.wasm
	cp tools/bridge.wasm tools/js_runtime/bridge.wasm
	cd playground && TWINKLE_LOCAL=1 npx vite

# Run the Vector benchmark suite (RRB Gate B baselines). See boot/bench/README.md.
# Pass BENCH=<name> to run a single benchmark, e.g. `make bench BENCH=concat_prepend`.
BENCH ?=
bench: target/twk
	@for f in $(if $(BENCH),boot/bench/$(BENCH).tw,$(sort $(wildcard boot/bench/*.tw))); do \
		printf '\n==> %s\n' "$$f"; \
		target/twk run "$$f" || exit 1; \
	done

bench-guard: target/twk
	python3 tools/check_vector_scaling.py

# Format all .tw source files owned by boot, excluding generated core_lib.
fmt: target/twk
	@find boot -name '*.tw' -not -path 'boot/lib/module/core_lib.tw' | xargs target/twk fmt

clean:
	cargo clean
	rm -rf target/boot.wasm target/boot-stage1.wasm target/twk target/deno-assets

# ---------------------------------------------------------------------------
# npm package (@twinkle-lang/twinkle)
# ---------------------------------------------------------------------------

# Stage a self-contained npm package into target/npm/ and build the tarball.
# Depends on a fresh self-hosted payload.
npm-pack: $(STAGE2_WASM) tools/build_npm_pkg.sh tools/npm/package.json tools/npm/README.md $(wildcard tools/js_runtime/*.mjs)
	tools/build_npm_pkg.sh
	cd target/npm && npm pack

# Publish the staged package to npm (requires `npm login` and the
# @twinkle-lang organization to exist).
npm-publish: $(STAGE2_WASM) tools/build_npm_pkg.sh tools/npm/package.json tools/npm/README.md $(wildcard tools/js_runtime/*.mjs)
	tools/build_npm_pkg.sh
	cd target/npm && npm publish

# Run the JS runtime/lib/CLI test suite (needs target/boot.wasm present).
npm-test: $(STAGE2_WASM)
	node --test tools/js_runtime/*.test.mjs
