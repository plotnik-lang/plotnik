# Grammar-Based Query Validation

Validate that query structural patterns (sequences, anchors, parent-child relationships) are actually derivable from the grammar's production rules.

## Problem Statement

Current validation uses `node-types.json`, which is a **lossy summary**:

```json
{
  "type": "function_declaration",
  "fields": {
    "name": { "types": [{"type": "identifier"}] },
    "body": { "types": [{"type": "statement_block"}] }
  }
}
```

This tells you "identifier can be a child" but loses **sequence information**.

The actual grammar (`grammar.json`) has production rules:

```json
{
  "type": "SEQ",
  "members": [
    {"type": "CHOICE", "members": [{"type": "STRING", "value": "async"}, {"type": "BLANK"}]},
    {"type": "STRING", "value": "function"},
    {"type": "FIELD", "name": "name", "content": {"type": "SYMBOL", "name": "identifier"}},
    {"type": "SYMBOL", "name": "_call_signature"},
    {"type": "FIELD", "name": "body", "content": {"type": "SYMBOL", "name": "statement_block"}}
  ]
}
```

**Query**: `(function_declaration . (identifier) @name)` — first-child anchor on identifier

**Reality**: First children are `"async"` (optional) then `"function"` keyword. Identifier is 3rd.

**Verdict**: Pattern is **structurally impossible**. But `node-types.json` can't detect this.

## Grammar JSON Structure

Tree-sitter's `grammar.json` uses these combinators:

| Type | Meaning |
|------|---------|
| `SEQ` | Sequence of members (order matters) |
| `CHOICE` | Alternatives (any branch) |
| `REPEAT` | Zero or more (`*`) |
| `REPEAT1` | One or more (`+`) |
| `SYMBOL` | Reference to another rule |
| `STRING` | Literal token (becomes anonymous node) |
| `PATTERN` | Regex for token |
| `FIELD` | Named child |
| `BLANK` | Epsilon (empty) |
| `PREC` / `PREC_LEFT` / `PREC_RIGHT` | Precedence wrappers |
| `ALIAS` | Rename a node |
| `TOKEN` / `IMMEDIATE_TOKEN` | Tokenization control |

### Example: `array`

```json
{
  "type": "SEQ",
  "members": [
    {"type": "STRING", "value": "["},
    {"type": "CHOICE", "members": [
      {"type": "SEQ", "members": [
        {"type": "CHOICE", "members": [{"type": "SYMBOL", "name": "expression"}, {"type": "BLANK"}]},
        {"type": "REPEAT", "content": {"type": "SEQ", "members": [
          {"type": "STRING", "value": ","},
          {"type": "CHOICE", "members": [{"type": "SYMBOL", "name": "expression"}, {"type": "BLANK"}]}
        ]}}
      ]},
      {"type": "BLANK"}
    ]},
    {"type": "STRING", "value": "]"}
  ]
}
```

Query `(array . (identifier))` with first-child anchor: **impossible** — `"["` is always first.

## Proposed Architecture

```
┌─────────────────┐
│  grammar.json   │
└────────┬────────┘
         ▼
┌─────────────────┐
│ Grammar IR      │  Parse SEQ/CHOICE/REPEAT/FIELD/SYMBOL/STRING/BLANK
└────────┬────────┘
         ▼
┌─────────────────┐
│ Visibility      │  Which elements produce visible tree nodes?
│ Analysis        │  - SYMBOL(name) → visible if not `_` prefixed
│                 │  - STRING("x") → anonymous node (visible)
│                 │  - _hidden_rule → inline its children
└────────┬────────┘
         ▼
┌─────────────────┐
│ Child Sequence  │  For each node type, build NFA accepting
│ NFA             │  valid child sequences
└────────┬────────┘
         ▼
┌─────────────────┐
│ Pattern Match   │  Query pattern → path constraints
│                 │  Check: ∃ path through NFA matching pattern?
└─────────────────┘
```

### 1. Grammar IR

```rust
enum GrammarExpr {
    Seq(Vec<GrammarExpr>),
    Choice(Vec<GrammarExpr>),
    Repeat(Box<GrammarExpr>),      // *
    Repeat1(Box<GrammarExpr>),     // +
    Optional(Box<GrammarExpr>),    // implicit from CHOICE with BLANK
    Symbol(String),                 // reference to rule
    String(String),                 // literal token
    Pattern(String),                // regex token
    Field { name: String, content: Box<GrammarExpr> },
    Blank,                          // epsilon
    Prec { value: i32, content: Box<GrammarExpr> },
    Alias { value: String, content: Box<GrammarExpr> },
}
```

### 2. Visibility Analysis

Determine which grammar elements produce visible tree nodes:

- **Named rules** (`identifier`, `function_declaration`): Produce named nodes
- **Hidden rules** (`_call_signature`, `_expression`): Inlined, don't produce nodes themselves
- **STRING values** (`"function"`, `"["`): Produce anonymous nodes
- **FIELD**: Just names a child, doesn't affect visibility

Hidden rules must be recursively inlined to get the actual child sequence.

### 3. Child Sequence NFA

For each visible node type, build an NFA:

- **States**: Positions in the flattened production rule
- **Transitions**: Node types (named and anonymous)
- **CHOICE**: ε-transitions to each branch
- **REPEAT**: Loop transition back to start state
- **BLANK**: ε-transition to next state

Example for `function_declaration`:

```
[0] ──"async"?──► [1] ──"function"──► [2] ──identifier──► [3] ──(params)──► [4] ──statement_block──► [5]
     └──────ε──────┘
```

### 4. Pattern Matching

Query patterns become path constraints on the NFA:

- **Regular child**: Must match a transition
- **Gap (no anchor)**: Allow any path between
- **First-child anchor (`. x`)**: x must be reachable from start with no prior named nodes
- **Last-child anchor (`x .`)**: x must reach accepting state with no following named nodes
- **Adjacent anchor (`x . y`)**: y immediately follows x (no intermediate named transitions)

### 5. Validation Algorithm

```
fn validate_sequence(parent: NodeTypeId, pattern: &QueryPattern) -> Result<(), Diagnostic> {
    let nfa = build_nfa(parent);
    let constraints = pattern_to_constraints(pattern);

    if !nfa.satisfies(constraints) {
        return Err(Diagnostic::ImpossibleSequence {
            parent,
            pattern,
            reason: explain_failure(nfa, constraints),
        });
    }
    Ok(())
}
```

## Complications

### Hidden Rule Inlining

`_call_signature` doesn't produce a node, but its children do:

```
_call_signature = SEQ(formal_parameters, type_annotation?)
```

When validating `function_declaration`, we inline this to get:

```
SEQ("async"?, "function", identifier, formal_parameters, type_annotation?, statement_block)
```

### Extras (Trivia)

The grammar's `extras` field lists nodes that can appear anywhere (comments, whitespace):

```json
"extras": [
  {"type": "SYMBOL", "name": "comment"},
  {"type": "PATTERN", "value": "\\s"}
]
```

These are handled by trivia-skipping in anchors, not by the NFA.

### Supertypes

`SYMBOL("expression")` matches any expression subtype. The NFA transition accepts any node that is a subtype of `expression`.

### Partial Matching

Query patterns don't match ALL children — they match a subsequence:

- Query `{(a) (c)}` matches actual children `[a, b, c]` (b skipped)
- The NFA must allow "gap" transitions between pattern elements

### Anchor Semantics

| Pattern | Meaning |
|---------|---------|
| `(P . (x))` | x is first child of P (or first after trivia for named nodes) |
| `(P (x) .)` | x is last child of P |
| `(P (x) . (y))` | y immediately follows x (no named nodes between) |
| `(P {. (x) (y) .})` | x is first AND y is last in the sequence |

## Diagnostics

```
error[E0XX]: impossible child sequence
  --> query.ptk:5:3
   |
 5 |   (function_declaration . (identifier) @name)
   |                         ^^^^^^^^^^^^^^^^^
   |
   = note: `identifier` cannot be the first child of `function_declaration`
   = note: the grammar requires `"function"` keyword before the name
   = help: remove the first-child anchor: `(function_declaration (identifier) @name)`
```

## Effort Estimate

| Component | LOC | Notes |
|-----------|-----|-------|
| Grammar JSON parser | ~400 | Serde, known schema |
| Grammar IR | ~300 | Recursive enum |
| Visibility analysis | ~300 | Inline hidden rules |
| NFA construction | ~500 | Per node type |
| Pattern matching | ~400 | Path satisfiability |
| Integration | ~200 | Hook into link.rs |
| Tests | ~500 | Edge cases |
| **Total** | **~2600** | |

## Build-time vs Runtime

**Option 1: Runtime**
- Load `grammar.json` at link time
- More flexible (supports dynamic languages)
- Slower startup, more memory

**Option 2: Build-time**
- Pre-compile grammar to static NFA tables in `build.rs`
- Faster, but larger binaries
- Matches current `node-types.json` approach

Recommendation: Start with runtime, optimize to build-time if needed.

## Open Questions

1. **How to handle recursive rules?** Some rules reference themselves (expressions containing expressions). NFAs must handle cycles.

2. **Grammar version matching?** Different tree-sitter versions may have different grammars. Need to ensure grammar matches the compiled parser.

3. **Error recovery nodes?** `ERROR` and `MISSING` nodes bypass normal grammar. Should they skip validation?

4. **Performance?** Building NFAs for all node types might be expensive. Consider lazy construction.

## Next Steps

1. Parse `grammar.json` into IR (new module in `plotnik-core` or `plotnik-lib`)
2. Implement visibility analysis and hidden rule inlining
3. Build NFA for a simple node type (e.g., `array`)
4. Validate a simple anchor pattern
5. Expand to full pattern matching
6. Integrate into `link.rs`
