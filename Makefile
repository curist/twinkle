.PHONY: help test boot-test rust-test lsp-smoke stage0 stage2 bundle-cli quick-bundle-cli cli clean

help:
	@printf 'Twinkle development targets:\n'
	@printf '  make test              Run Rust and boot compiler tests\n'
	@printf '  make boot-test         Run boot compiler test suite\n'
	@printf '  make rust-test         Run Rust test suite\n'
	@printf '  make lsp-smoke         Run framed stdio LSP smoke test against target/twk\n'
	@printf '  make stage0            Build the Rust stage0 compiler\n'
	@printf '  make stage2            Rebuild target/boot.wasm via self-host loop\n'
	@printf '  make bundle-cli        Rebuild stage2 payload and bundle target/twk\n'
	@printf '  make quick-bundle-cli  Bundle target/twk from existing target/boot.wasm\n'
	@printf '  make cli               Alias for bundle-cli\n'

# Fast day-to-day validation for boot compiler changes.
boot-test:
	tools/boot-test-fast.sh

rust-test:
	cargo test

test: rust-test boot-test

lsp-smoke:
	node tools/lsp_smoke.mjs

# Build the Rust stage0 compiler used to bootstrap the self-hosted compiler.
stage0:
	cargo build --release

# Refresh target/boot.wasm from current boot sources and verify the fixed point.
stage2: stage0
	tools/selfhost_loop.sh boot/main.tw

# Full bundled CLI rebuild. Use this after changing boot/main.tw or any code
# that should be embedded into ./target/twk.
bundle-cli: stage2
	tools/build_bun_cli.sh

cli: bundle-cli

# Rebundle the CLI from the existing target/boot.wasm without rebuilding the
# self-hosted payload. This is only correct when target/boot.wasm is already fresh.
quick-bundle-cli:
	tools/build_bun_cli.sh

clean:
	cargo clean
	rm -f target/boot.wasm target/boot-stage1.wasm target/twk target/twk_cli_payload.mjs
