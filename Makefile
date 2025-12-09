.PHONY: check clippy test snapshots coverage coverage-lines coverage clean

check:
	@cargo check --workspace --all-targets

clippy:
	@cargo clippy --workspace --all-targets -- -D warnings

test:
	@cargo nextest run --no-fail-fast --hide-progress-bar --status-level none --failure-output final

snapshots:
	@cargo insta accept

coverage-lines:
	@cargo llvm-cov --package plotnik-lib --text --show-missing-lines 2>/dev/null | grep '\.rs: [0-9]' | sed 's|.*/crates/|crates/|'

coverage:
	@cargo +nightly llvm-cov --all-features --workspace --lcov --output-path lcov.info

fmt:
	@cargo fmt --quiet
	@npx -y prettier --list-different --write .

clean:
	@cargo clean
