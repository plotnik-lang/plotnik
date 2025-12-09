# ADR-0002: Prioritized Diagnostics System

- **Status**: Accepted
- **Date**: 2025-12-08 (retrospective)

## Context

A single syntax error can cause many cascading downstream errors, overwhelming the user. Our goal is to present only the most relevant, actionable feedback.

## Decision

We implemented a diagnostics system with priority-based suppression.

- **Priority**: A central `DiagnosticKind` enum defines all possible diagnostics, ordered by priority.
- **Suppression**: When multiple diagnostics overlap, a filtering process suppresses lower-priority ones, effectively hiding noise and showing the likely root cause.
- **Formatting**: The `annotate-snippets` crate renders rich, user-friendly error messages with source context.

## Consequences

- **Positive**: Provides high-quality, actionable feedback by eliminating distracting cascading errors. The system is decoupled and independently testable.
- **Negative**: The suppression logic adds complexity and requires careful maintenance and tuning to remain effective.
