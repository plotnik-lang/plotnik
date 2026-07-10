.PHONY: check clippy test bench coverage coverage-lines check-wasm wasm-web clean

LLVM_PREFIX ?= /opt/homebrew/opt/llvm
WASM_CC ?= $(LLVM_PREFIX)/bin/clang
WASM_AR ?= $(LLVM_PREFIX)/bin/llvm-ar

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
	@cargo nextest run \
		--no-fail-fast \
		--show-progress only \
		--status-level fail \
		--failure-output final $(FILTER)

shot:
	@# See AGENTS.md for diagnostic guidelines
	@# SHOT=1 accepts the golden-fixture suite (tests/0N-*); TRYBUILD=overwrite
	@# refreshes the macro_ui .stderr goldens; cargo insta accept does the rest.
	@SHOT=1 TRYBUILD=overwrite cargo nextest run \
		--no-fail-fast \
		--hide-progress-bar \
		--status-level none \
		--failure-output final $(FILTER) \
		|| true
	@cargo insta accept
	@cargo run --quiet --package plotnik-tests --bin export-conformance
	@cargo nextest run \
		--no-fail-fast \
		--hide-progress-bar \
		--status-level none \
		--failure-output final $(FILTER)

bench:
	@cargo bench \
		--package plotnik-tests \
		--bench vm \
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
		--package plotnik-lib \
		--target wasm32-unknown-unknown
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
		--output-path lcov.info

fmt:
	@cargo fmt --quiet
	@npx -y prettier --list-different --write .

clean:
	@cargo clean
