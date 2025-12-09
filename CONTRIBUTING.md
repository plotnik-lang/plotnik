# Contributing to Plotnik

Thank you for your interest in contributing to Plotnik.

## License Agreement

By contributing to this project, you agree that your contributions will be licensed under the [Apache License 2.0](LICENSE).

This means:
- You grant a perpetual, worldwide, royalty-free patent license to all users
- Your code can be used in commercial products without restriction
- You retain copyright to your contributions

## Before Contributing

Read [`AGENTS.md`](AGENTS.md) — it's our constitution and contains:
- Project ethos and design principles
- Architecture Decision Records (ADRs)
- Code and testing conventions
- Project structure overview

## Code Conventions

### Style
- Early returns over nested logic
- Comments explain "why," not "what"
- Code is written for senior engineers, not juniors

### Testing
- Tests live in `foo_tests.rs`, included via `#[cfg(test)] mod foo_tests;`
- Use `insta` for snapshot testing
- AAA pattern (Arrange-Act-Assert) separated by blank lines
- Never write snapshots manually — use `@""` and `cargo insta accept`

### Running Tests
```sh
make test              # Run all tests
make snapshots         # Accept snapshot changes
make coverage-lines    # Check coverage per file
```

### CLI Testing
The `debug` command is your first testing tool:
```sh
cargo run -p plotnik-cli -- debug -q '(identifier) @id'
cargo run -p plotnik-cli -- debug -q 'YourQuery' --only-symbols
```

## Submitting Changes

1. **Fork and branch** — Create a feature branch from `main`
2. **Write tests** — New features need tests, bug fixes need regression tests
3. **Run checks** — Ensure `make test` passes
4. **Keep commits atomic** — One logical change per commit
5. **Write clear commit messages** — Explain the "why" behind the change
6. **Submit a PR** — Reference any related issues

## Architecture Changes

If your contribution involves a significant architectural decision:
- Create an ADR (Architecture Decision Record) in `docs/adr/`
- Follow the template in `AGENTS.md`
- Number it sequentially: `ADR-XXXX-short-title.md`

## Questions?

Open an issue for discussion before starting major work. We prefer to align on approach before you invest significant time.