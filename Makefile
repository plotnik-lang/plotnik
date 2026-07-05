.PHONY: check clippy test bench coverage coverage-lines clean

check:
	@cargo check \
		--workspace \
		--all-targets

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
	@# SHOT=1 accepts the golden-fixture suite (tests/0N-*); cargo insta accept does the rest.
	@SHOT=1 cargo nextest run \
		--no-fail-fast \
		--hide-progress-bar \
		--status-level none \
		--failure-output final $(FILTER) \
		|| true
	@cargo insta accept
	@cargo nextest run \
		--no-fail-fast \
		--hide-progress-bar \
		--status-level none \
		--failure-output final $(FILTER)

bench:
	@cargo bench \
		--package plotnik-lib \
		--bench vm \
		-- $(FILTER)

coverage-lines:
	@cargo llvm-cov \
		--package plotnik-lib \
		--text \
		--show-missing-lines \
		2>/dev/null | grep '\.rs: [0-9]' | sed 's|.*/crates/|crates/|'

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
