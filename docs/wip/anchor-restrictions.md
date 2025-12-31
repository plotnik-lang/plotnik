# Anchor Restrictions Implementation Plan

## Problem Statement

Anchors (`.`) are parsed but:
1. **Ignored in bytecode** - compiler drops anchors entirely
2. **Create garbage in alternations** - `[(a) . (b)]` creates empty branches
3. **No semantic validation** - boundary anchors without context are silently accepted

## Current Behavior (CLI Tests)

| Pattern | Parser | AST | Should Be |
|---------|--------|-----|-----------|
| `Q = . (a)` | Two defs created, error | Anchor absorbed | Error: anchor at def level |
| `Q = {. (a)}` | ✓ Parses | ✓ Anchor in Seq | Error: boundary without context |
| `Q = {(a) . (b)}` | ✓ Parses | ✓ Anchor between | ✓ Valid |
| `Q = (p . (a))` | ✓ Parses | ✓ In node | ✓ Valid |
| `Q = [(a) . (b)]` | ⚠️ Parses | Empty branch created! | Error: anchor in alternation |

## Two-Level Solution

### Level 1: Parser — Reject Anchors in Alternations

**Problem**: In `parse_alt_children()`, anchors create empty branches because `.` is in `EXPR_FIRST_TOKENS` but produces an `Anchor` node, not an expression.

**Fix**: In `structures.rs:parse_alt_children()`, check for `Dot` before `EXPR_FIRST_TOKENS` and emit error.

**File**: `crates/plotnik-lib/src/parser/grammar/structures.rs`

```rust
// In parse_alt_children(), before line 280:
if self.currently_is(SyntaxKind::Dot) {
    let span = self.current_span();
    self.diagnostics
        .report(self.source_id, DiagnosticKind::AnchorInAlternation, span)
        .emit();
    self.skip_token();  // Skip the dot, don't create empty branch
    continue;
}
if self.currently_is_one_of(EXPR_FIRST_TOKENS) {
    // ... existing code
}
```

**New Diagnostic** in `diagnostics/message.rs`:
```rust
// Add after InvalidSeparator (line 34):
AnchorInAlternation,

// In fallback_message():
Self::AnchorInAlternation => "anchors cannot appear directly in alternations",

// In default_hint():
Self::AnchorInAlternation => Some("use `[{(a) . (b)} (c)]` to anchor within a branch"),
```

**Error output**:
```
error: anchors cannot appear directly in alternations
  |
1 | Q = [(a) . (b)]
  |          ^
  |
help: use `[{(a) . (b)} (c)]` to anchor within a branch
```

---

### Level 2: Analyzer — Validate Anchor Context

**Problem**: `{. (a)}` at definition level parses correctly but is semantically invalid — boundary anchors need parent node context.

**Rule**:
- **Boundary anchors** (position 0 or last in sequence) require parent named node context
- **Interior anchors** (between items) are always valid
- **Named node children** always have context (the node provides it)

**New File**: `crates/plotnik-lib/src/query/anchors.rs`

```rust
//! Semantic validation for anchor placement.

use super::visitor::{Visitor, walk, walk_named_node, walk_seq_expr};
use crate::SourceId;
use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::parser::ast::{NamedNode, SeqExpr, SeqItem, Root};

pub fn validate_anchors(source_id: SourceId, ast: &Root, diag: &mut Diagnostics) {
    let mut visitor = AnchorValidator {
        diag,
        source_id,
        in_named_node: false,
    };
    visitor.visit(ast);
}

struct AnchorValidator<'a> {
    diag: &'a mut Diagnostics,
    source_id: SourceId,
    in_named_node: bool,
}

impl Visitor for AnchorValidator<'_> {
    fn visit_named_node(&mut self, node: &NamedNode) {
        let prev = self.in_named_node;
        self.in_named_node = true;

        // Anchors inside named node children are always valid
        // (the node provides first/last/adjacent context)
        walk_named_node(self, node);

        self.in_named_node = prev;
    }

    fn visit_seq_expr(&mut self, seq: &SeqExpr) {
        let items: Vec<_> = seq.items().collect();
        let len = items.len();

        for (i, item) in items.iter().enumerate() {
            if let SeqItem::Anchor(anchor) = item {
                let is_boundary = i == 0 || i == len - 1;

                if is_boundary && !self.in_named_node {
                    self.diag
                        .report(
                            self.source_id,
                            DiagnosticKind::AnchorWithoutContext,
                            anchor.text_range(),
                        )
                        .emit();
                }
            }
        }

        walk_seq_expr(self, seq);
    }
}
```

**New Diagnostic** in `diagnostics/message.rs`:
```rust
// Add after AnchorInAlternation:
AnchorWithoutContext,

// In fallback_message():
Self::AnchorWithoutContext => "boundary anchor requires parent node context",

// In default_hint():
Self::AnchorWithoutContext => Some("wrap in a named node: `(parent . (child))`"),
```

**Error output**:
```
error: boundary anchor requires parent node context
  |
1 | Q = {. (a)}
  |      ^
  |
help: wrap in a named node: `(parent . (child))`
```

**Integration** in `query/query.rs` (after line 75):
```rust
validate_alt_kinds(source.id, &res.ast, &mut diag);
validate_anchors(source.id, &res.ast, &mut diag);  // NEW
```

---

## Valid vs Invalid Patterns (Final)

| Pattern | Valid? | Reason |
|---------|--------|--------|
| `Q = . (a)` | ❌ | Parser: creates separate def (existing behavior) |
| `Q = {. (a)}` | ❌ | Analyzer: boundary without context |
| `Q = {(a) .}` | ❌ | Analyzer: boundary without context |
| `Q = {(a) . (b)}` | ✓ | Interior anchor, both sides defined |
| `Q = (p . (a))` | ✓ | Inside node, first child anchor |
| `Q = (p (a) .)` | ✓ | Inside node, last child anchor |
| `Q = (p (a) . (b))` | ✓ | Inside node, adjacent siblings |
| `Q = (p {. (a)})` | ✓ | Seq inside node, context from p |
| `Q = (p {(a) . (b)})` | ✓ | Interior anchor in nested seq |
| `Q = [(a) . (b)]` | ❌ | Parser: anchor in alternation |
| `Q = [{(a) . (b)} (c)]` | ✓ | Anchor inside seq branch |

---

## Documentation Update

Update `docs/lang-reference.md` section "## Anchors" to add:

```markdown
### Anchor Placement Rules

Anchors require context to be meaningful:

**Valid positions:**
- Inside a named node's children: `(parent . (first))`, `(parent (a) . (b))`
- Between items in a sequence inside a node: `(parent {(a) . (b)})`

**Invalid positions:**
- At definition level: `Q = . (a)` ❌
- At sequence boundaries without parent node: `Q = {. (a)}` ❌
- Directly in alternations: `Q = [(a) . (b)]` ❌

The rule: **boundary anchors need a parent named node to provide context**
(first child, last child, or adjacent sibling semantics).

Interior anchors (between sequence items) are always valid because both sides
are explicitly defined.
```

---

## Files to Modify

### Parser Level
1. `crates/plotnik-lib/src/parser/grammar/structures.rs` — Check for Dot before EXPR_FIRST_TOKENS in alternation parsing
2. `crates/plotnik-lib/src/diagnostics/message.rs` — Add `AnchorInAlternation`
3. `crates/plotnik-lib/src/parser/tests/` — Add tests for anchor-in-alt error

### Analyzer Level
4. `crates/plotnik-lib/src/query/anchors.rs` — NEW: semantic validation
5. `crates/plotnik-lib/src/query/anchors_tests.rs` — NEW: tests
6. `crates/plotnik-lib/src/query/mod.rs` — Export anchors module
7. `crates/plotnik-lib/src/query/query.rs` — Call `validate_anchors()`
8. `crates/plotnik-lib/src/diagnostics/message.rs` — Add `AnchorWithoutContext`

### Documentation
9. `docs/lang-reference.md` — Add "Anchor Placement Rules" section

---

## Test Cases

### Parser Tests (`parser/tests/recovery/anchors_tests.rs`)

```rust
#[test]
fn anchor_in_alternation_error() {
    let input = "Q = [(a) . (b)]";
    let res = Query::expect_invalid(input);
    insta::assert_snapshot!(res.dump_diagnostics(), @r#"
    error: anchors cannot appear directly in alternations
      |
    1 | Q = [(a) . (b)]
      |          ^
      |
    help: use `[{(a) . (b)} (c)]` to anchor within a branch
    "#);
}

#[test]
fn anchor_in_seq_inside_alt_ok() {
    let input = "Q = [{(a) . (b)} (c)]";
    let _ = Query::expect_valid_ast(input);
}
```

### Analyzer Tests (`query/anchors_tests.rs`)

```rust
#[test]
fn boundary_anchor_without_context() {
    let input = "Q = {. (a)}";
    let res = Query::expect_invalid(input);
    insta::assert_snapshot!(res.dump_diagnostics(), @r#"
    error: boundary anchor requires parent node context
      |
    1 | Q = {. (a)}
      |      ^
      |
    help: wrap in a named node: `(parent . (child))`
    "#);
}

#[test]
fn boundary_anchor_with_context_ok() {
    let input = "Q = (parent {. (a)})";
    let _ = Query::expect_valid_ast(input);
}

#[test]
fn interior_anchor_always_valid() {
    let input = "Q = {(a) . (b)}";
    let _ = Query::expect_valid_ast(input);
}

#[test]
fn anchor_in_named_node_ok() {
    let input = "Q = (parent . (first) (second) .)";
    let _ = Query::expect_valid_ast(input);
}
```

---

## Implementation Order

1. **Parser fix** (30 min): Add `AnchorInAlternation` diagnostic, check in `parse_alt_children()`
2. **Analyzer validation** (45 min): Create `anchors.rs`, add `AnchorWithoutContext`, integrate
3. **Tests** (30 min): Add parser and analyzer tests
4. **Documentation** (15 min): Update lang-reference.md

Total: ~2 hours

---

## Future Work (Not in Scope)

After anchor restrictions are in place, the next step is implementing anchor **compilation**:
- Use `items()` instead of `children()` in compiler
- Fix sibling navigation (Down → Next for non-first children)
- Generate `NextSkip`/`NextExact`/`DownSkip`/`DownExact` based on anchor presence
- Handle `UpExact`/`UpSkipTrivia` for last-child anchors

This is tracked separately as bytecode emission work.
