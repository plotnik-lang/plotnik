# ADR-0001: Hand-written Parser with Rowan

- **Status**: Accepted
- **Date**: 2025-12-08 (retrospective)

## Context

We need a resilient parser with excellent, user-friendly diagnostics and fine-grained error recovery. Parser generators like `chumsky` were considered but offer insufficient control.

## Decision

We implemented a hand-written recursive descent parser.

- **Lexer**: `logos` for zero-copy tokenization.
- **CST**: `rowan` to build a lossless Concrete Syntax Tree, preserving all source text and trivia.
- **AST**: A typed AST wrapper provides a clean API for semantic analysis.

## Consequences

- **Positive**: Full control over error recovery, enabling high-quality diagnostics. The lossless CST is ideal for accurate error reporting and future tooling (e.g., formatters).
- **Negative**: Higher initial development effort and complexity compared to parser generators.
