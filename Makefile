.PHONY: check clippy test snapshots coverage coverage-lines coverage clean

check:
	@cargo check --workspace --all-targets

clippy:
	@cargo clippy --workspace --all-targets -- -D warnings

test:
	@cargo nextest run --no-fail-fast --hide-progress-bar --status-level none --failure-output final

shot:
	@# See AGENTS.md for diagnostic guidelines
	@# SHOT=1 accepts the golden-fixture suite (tests/0N-*); cargo insta accept does the rest.
	@SHOT=1 cargo nextest run --no-fail-fast --hide-progress-bar --status-level none --failure-output final || true
	@cargo insta accept
	@cargo nextest run --no-fail-fast --hide-progress-bar --status-level none --failure-output final

# Run the golden-fixture suite. F=<filter> scopes it: F=06-vm (stage),
# F=06-vm/captures (folder), or F=anchors (one construct across every stage).
snapshots:
	cargo nextest run -p plotnik-lib --test snapshots $(F)

coverage-lines:
	@cargo llvm-cov --package plotnik-lib --text --show-missing-lines 2>/dev/null | grep '\.rs: [0-9]' | sed 's|.*/crates/|crates/|'

coverage:
	@cargo +nightly llvm-cov --all-features --workspace --lcov --output-path lcov.info

fmt:
	@cargo fmt --quiet
	@npx -y prettier --list-different --write .

clean:
	@cargo clean
