.PHONY: check clippy test test-arborium codegen-rust lint-codegen-rust test-codegen-rust bench coverage coverage-lines check-wasm wasm-web clean

LLVM_PREFIX ?= /opt/homebrew/opt/llvm
WASM_CC ?= $(LLVM_PREFIX)/bin/clang
WASM_AR ?= $(LLVM_PREFIX)/bin/llvm-ar
BENCH ?= vm
CODEGEN_TARGET_DIR ?= $(if $(strip $(CARGO_TARGET_DIR)),$(abspath $(CARGO_TARGET_DIR)),$(CURDIR)/target)
CODEGEN_RUST_DIR := $(CURDIR)/crates/plotnik-tests/codegen/rust
CODEGEN_RUST_MANIFEST := $(CODEGEN_RUST_DIR)/Cargo.toml
CODEGEN_RUST_TESTS := $(CODEGEN_RUST_DIR)/tests

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

codegen-rust:
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
	@CARGO_TARGET_DIR="$(CODEGEN_TARGET_DIR)" \
		"$(CODEGEN_TARGET_DIR)/debug/plotnik-codegen-tests" rust \
		--plotnik "$(CODEGEN_TARGET_DIR)/debug/plotnik" \
		$(if $(strip $(FILTER)),--filter "$(FILTER)",)

lint-codegen-rust:
	@test -d "$(CODEGEN_RUST_TESTS)" || { \
		echo "generated Rust tests are missing; run 'make codegen-rust' first" >&2; \
		exit 1; \
	}
	@cargo clippy \
		--locked \
		--package plotnik-tests \
		--bin plotnik-codegen-tests \
		--features codegen-tests \
		--target-dir "$(CODEGEN_TARGET_DIR)" \
		-- \
		-D warnings
	@cargo fmt \
		--manifest-path "$(CODEGEN_RUST_MANIFEST)" \
		--check
	@cargo clippy \
		--locked \
		--manifest-path "$(CODEGEN_RUST_MANIFEST)" \
		--target-dir "$(CODEGEN_TARGET_DIR)" \
		--all-targets \
		-- \
		-D warnings

test-codegen-rust:
	@test -d "$(CODEGEN_RUST_TESTS)" || { \
		echo "generated Rust tests are missing; run 'make codegen-rust' first" >&2; \
		exit 1; \
	}
	@cargo test \
		--locked \
		--package plotnik-tests \
		--bin plotnik-codegen-tests \
		--features codegen-tests \
		--target-dir "$(CODEGEN_TARGET_DIR)" \
		--quiet
	@cargo test \
		--locked \
		--manifest-path "$(CODEGEN_RUST_MANIFEST)" \
		--target-dir "$(CODEGEN_TARGET_DIR)" \
		--no-fail-fast

shot:
	@# See AGENTS.md for diagnostic guidelines
	@# SHOT=1 updates custom inline snapshots; TRYBUILD=overwrite updates .stderr snapshots.
	@# The first run also records pending Insta snapshots, which cargo insta then accepts.
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
	@cargo clean --manifest-path "$(CODEGEN_RUST_MANIFEST)"
	@rm -rf \
		"$(CODEGEN_RUST_TESTS)" \
		"$(CODEGEN_RUST_DIR)"/tests.pending-*
