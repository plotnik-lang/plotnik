.PHONY: check clippy test test-arborium bench coverage coverage-lines check-wasm wasm-web clean

LLVM_PREFIX ?= /opt/homebrew/opt/llvm
WASM_CC ?= $(LLVM_PREFIX)/bin/clang
WASM_AR ?= $(LLVM_PREFIX)/bin/llvm-ar
BENCH ?= vm

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

test-arborium:
	@diff -ru crates/plotnik-rt/src crates/plotnik-rt-arborium/src
	@diff -ru crates/plotnik/src crates/plotnik-arborium/src
	@cargo test \
		--manifest-path crates/plotnik-rt-arborium/Cargo.toml \
		--all-targets \
		--all-features \
		--quiet
	@cargo clippy \
		--manifest-path crates/plotnik-rt-arborium/Cargo.toml \
		--all-targets \
		--all-features \
		-- \
		-D warnings
	@cargo clippy \
		--manifest-path crates/plotnik-arborium/Cargo.toml \
		--all-targets \
		--all-features \
		-- \
		-D warnings
	@mkdir -p target/arborium-smoke
	@cargo run \
		--package plotnik-cli \
		--locked \
		--quiet \
		-- generate examples/arborium/query.ptk \
		--target rust \
		--lang javascript \
		--output target/arborium-smoke/generated.rs
	@cmp examples/arborium/src/generated.rs target/arborium-smoke/generated.rs
	@cargo run \
		--manifest-path examples/arborium/Cargo.toml \
		--locked \
		--quiet

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
