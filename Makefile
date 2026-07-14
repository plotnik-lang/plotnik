.PHONY: check clippy test test-codegen-rust bench coverage coverage-lines check-wasm wasm-web clean

LLVM_PREFIX ?= /opt/homebrew/opt/llvm
WASM_CC ?= $(LLVM_PREFIX)/bin/clang
WASM_AR ?= $(LLVM_PREFIX)/bin/llvm-ar
BENCH ?= vm
CODEGEN_TARGET_DIR ?= $(if $(strip $(CARGO_TARGET_DIR)),$(abspath $(CARGO_TARGET_DIR)),$(CURDIR)/target)

check:
	@cargo check \
		--workspace \
		--all-targets
	@cargo check \
		--package plotnik-lib \
		--no-default-features

clippy:
	@cargo clippy \
		--workspace \
		--all-targets \
		-- \
		-D warnings

test:
	@cargo test \
		--workspace \
		--lib \
		--bins \
		--tests \
		--no-fail-fast \
		--quiet \
		-- \
		$(FILTER)

test-codegen-rust:
	@cargo build \
		--locked \
		--package plotnik-cli \
		--no-default-features \
		--target-dir "$(CODEGEN_TARGET_DIR)"
	@cargo build \
		--locked \
		--package plotnik-tests \
		--bin plotnik-codegen-tests \
		--features codegen-tests \
		--target-dir "$(CODEGEN_TARGET_DIR)"
	@cargo clippy \
		--locked \
		--package plotnik-tests \
		--bin plotnik-codegen-tests \
		--features codegen-tests \
		--target-dir "$(CODEGEN_TARGET_DIR)" \
		-- \
		-D warnings
	@cargo test \
		--locked \
		--package plotnik-tests \
		--bin plotnik-codegen-tests \
		--features codegen-tests \
		--target-dir "$(CODEGEN_TARGET_DIR)" \
		--quiet
	@CARGO_TARGET_DIR="$(CODEGEN_TARGET_DIR)" \
		"$(CODEGEN_TARGET_DIR)/debug/plotnik-codegen-tests" rust \
		--plotnik "$(CODEGEN_TARGET_DIR)/debug/plotnik" \
		$(if $(strip $(FILTER)),--filter "$(FILTER)",)

shot:
	@# See AGENTS.md for diagnostic guidelines
	@# SHOT=1 accepts the golden-fixture suite (tests/0N-*); TRYBUILD=overwrite
	@# refreshes the macro_diagnostics .stderr goldens; cargo insta accept does the rest.
	@SHOT=1 TRYBUILD=overwrite cargo test \
		--workspace \
		--lib \
		--bins \
		--tests \
		--no-fail-fast \
		--quiet \
		-- \
		$(FILTER) \
		|| true
	@cargo insta accept
	@cargo test \
		--workspace \
		--lib \
		--bins \
		--tests \
		--no-fail-fast \
		--quiet \
		-- \
		$(FILTER)

bench:
	@cargo bench \
		--package plotnik-tests \
		--bench $(BENCH) \
		-- $(FILTER)

coverage-lines:
	@cargo llvm-cov \
		--workspace \
		--text \
		--show-missing-lines \
		2>/dev/null | grep '\.rs: [0-9]' | sed 's|.*/crates/|crates/|'

check-wasm:
	@CC_wasm32_unknown_unknown="$(WASM_CC)" \
		AR_wasm32_unknown_unknown="$(WASM_AR)" \
		cargo check \
		--package plotnik-wasm \
		--target wasm32-unknown-unknown

wasm-web:
	@CC_wasm32_unknown_unknown="$(WASM_CC)" \
		AR_wasm32_unknown_unknown="$(WASM_AR)" \
		cargo build \
		--package plotnik-wasm \
		--target wasm32-unknown-unknown \
		--release
	@wasm-bindgen \
		--target web \
		--out-dir web/src/lib/plotnik-wasm \
		target/wasm32-unknown-unknown/release/plotnik_wasm.wasm

coverage:
	@cargo +nightly llvm-cov \
		--all-features \
		--workspace \
		--lcov \
		--output-path lcov.info \
		-- \
		--skip macro_diagnostics

fmt:
	@cargo fmt --quiet
	@npx -y prettier --list-different --write .

clean:
	@cargo clean
