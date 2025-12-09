# The Plotnik ADR System

An ADR system documents important architectural decisions, their context, and their consequences. This helps maintain architectural consistency and provides valuable context for current and future contributors.

## 1. Location

As hinted at in your `AGENTS.md`, the best place for these is `docs/adr/`.

## 2. Naming Convention

Files should be named `ADR-XXXX-short-title-in-kebab-case.md`, where `XXXX` is a sequential number (e.g., `0001`, `0002`).

## 3. ADR Template

Create a file named `ADR-0000-template.md` in the `docs/adr/` directory with the following content. This makes it easy for anyone to start a new record.

```markdown
# ADR-XXXX: Title of the Decision

- **Status**: Proposed | Accepted | Deprecated | Superseded by [ADR-YYYY](ADR-YYYY-...)
- **Date**: YYYY-MM-DD

## Context

Describe the issue, problem, or driving force that led to this decision. What are the constraints and requirements? What is the scope of this decision? This section should be understandable to someone without deep project knowledge.

## Decision

Clearly and concisely state the decision that was made. This is the "what," not the "why."

## Consequences

This is the most critical section. Describe the results, outcomes, and trade-offs of the decision.

### Positive Consequences

- What benefits does this decision provide?
- How does it align with the project's goals (e.g., resilience, user experience, performance)?

### Negative Consequences

- What are the drawbacks or costs?
- What trade-offs were made?
- What future challenges might this decision introduce?

### Considered Alternatives

- **Alternative 1:** A brief description of a rejected option.
  - _Pros_: Why was it considered?
  - _Cons_: Why was it rejected?
- **Alternative 2:** ...
```
