# Node Iteration Strategy for Query Execution

This document describes how tree-sitter's node access works internally and the recommended approach for plotnik's query execution engine.

## Matching Model

Plotnik matches children sequentially by position. Field constraints add an additional requirement—the child at that position must ALSO have the specified field:

```
(binary_expression
  left: (identifier) @x    ; 1st child: must be identifier AND have field "left"
  right: (number) @y       ; 2nd child: must be number AND have field "right"
)
```

This is positional matching with field filtering, not independent field lookups.

## Tree-sitter API

### TreeCursor (Iteration)

```rust
cursor.goto_first_child() -> bool
cursor.goto_next_sibling() -> bool
cursor.goto_parent() -> bool
cursor.node() -> Node
cursor.field_id() -> Option<FieldId>   // field of CURRENT child
cursor.field_name() -> Option<&str>
```

During iteration, `field_id()` is O(field_map_size), typically 1-5 entries.

### Node (Random Access)

```rust
node.child_by_field_name(name) -> Option<Node>
node.child_by_field_id(id) -> Option<Node>
```

### Language (Field Metadata)

```rust
language.field_id_for_name(name) -> Option<FieldId>
language.field_name_for_id(id) -> Option<&str>
```

## Internal: Field Map

Tree-sitter maintains a `field_map` per production (grammar rule):

```c
typedef struct {
  TSFieldId field_id;
  uint8_t child_index;    // structural child position
  bool inherited;         // from hidden child
} TSFieldMapEntry;
```

The `child_by_field_id` function:

1. Looks up `field_map_entries` for the node's `production_id`
2. If no entries for this field → returns null immediately (O(1))
3. Otherwise iterates children, skipping until reaching `field_map.child_index`

## Execution Strategy

### Negated Fields (Early Exit)

Check negated fields first via random access before iteration:

```rust
for negated_field in pattern.negated_fields {
    if node.child_by_field_id(negated_field).is_some() {
        return None; // pattern fails
    }
}
```

This is the only place random access helps:

- Field impossible per grammar → O(1) fail
- Field exists → found quickly, fail before iteration
- Field absent → we'd iterate anyway, no extra cost

### Sequential Matching

Iterate children with cursor, checking node type AND field constraints together:

```rust
let mut cursor = node.walk();
if !cursor.goto_first_child() {
    return pattern.children.is_empty().then_some(captures);
}

let mut pattern_idx = 0;
loop {
    let child = cursor.node();
    let field_id = cursor.field_id();

    if let Some(child_pattern) = pattern.children.get(pattern_idx) {
        // Check node type matches
        if !matches_type(child_pattern, child) {
            // scanning: try next sibling (unless anchored)
        }

        // Check field constraint if present
        if let Some(required_field) = child_pattern.field {
            if field_id != Some(required_field) {
                // field mismatch: try next sibling or fail
            }
        }

        // Both match: capture and advance pattern
        pattern_idx += 1;
    }

    if !cursor.goto_next_sibling() {
        break;
    }
}
```

## MVP Implementation

1. Check negated fields first (random access, early exit)
2. Iterate children with cursor (required for backtracking)
3. For each child, check node type AND field via `cursor.field_id()`
4. Handle scanning (gaps allowed) vs anchors (strict adjacency)

## Backtracking with Descendant Index

Quantifiers (`*`, `+`, `?`) and alternations (`[a b c]`) require backtracking when a branch fails. Tree-sitter provides an efficient checkpoint mechanism.

### The Problem

Cursor cloning (`TreeCursor::copy()`) is expensive:

- Heap allocation for internal stack
- O(depth) memcpy
- Requires explicit cleanup

### The Solution

Use `descendant_index` as a lightweight checkpoint:

```rust
// Save checkpoint (4 bytes)
let checkpoint: u32 = cursor.descendant_index();

// Try a branch...
if !try_match_branch(&mut cursor) {
    // Restore: rebuilds ancestor stack, no heap alloc
    cursor.goto_descendant(checkpoint);
    // Try next branch...
}
```

### Cost Analysis

- Save: O(1), returns `u32`
- Restore: O(depth), iterates to rebuild ancestor stack
- Memory: 4 bytes per checkpoint (vs ~32 bytes × depth for clone)

### Critical Constraint

The `descendant_index` is relative to the cursor's root node (the node it was constructed with).

Dangerous methods (invalidate saved indices):

- `reset(node)` - resets root, indices restart from 0
- `reset_to(cursor)` - safe only if source cursor shares same root

Safe methods (preserve indices):

- All `goto_*` methods: `goto_first_child`, `goto_last_child`, `goto_next_sibling`, `goto_previous_sibling`, `goto_parent`, `goto_descendant`, `goto_first_child_for_byte`, `goto_first_child_for_point`

Rule: create cursor once at tree root, never call `reset()`.

### Usage Pattern

```rust
let mut cursor = tree.root_node().walk();  // Create once at true root

fn match_quantifier(cursor: &mut TreeCursor, pattern: &Pattern) -> Option<Captures> {
    let checkpoint = cursor.descendant_index();

    // Try greedy match
    while matches(cursor, pattern) {
        cursor.goto_next_sibling();
    }

    // Backtrack if subsequent pattern fails
    if !matches_rest(cursor, rest_pattern) {
        cursor.goto_descendant(checkpoint);
        // Try non-greedy...
    }
}
```

## Future Optimizations

- Early termination when all pattern children satisfied
- Heuristics for choosing greedy vs non-greedy quantifier expansion
- Use `goto_first_child_for_byte` to skip ahead when byte offset is known from prior matches

## Key Insights

1. Fields are constraints ON positional matching, not independent lookups
2. `cursor.field_id()` during iteration is cheap (O(field_map_size), typically 1-5)
3. Random access (`child_by_field_id`) only useful for negated fields (early exit)
4. Cursor must be used for all navigation to preserve `descendant_index` for backtracking

## References

- `tree-sitter/lib/src/node.c`: `ts_node_child_by_field_id`
- `tree-sitter/lib/src/tree_cursor.c`: `ts_tree_cursor_current_field_id`
- `tree-sitter/lib/src/parser.h`: `TSFieldMapEntry`
- `tree-sitter/lib/src/language.h`: `ts_language_field_map`
- `tree-sitter/lib/binding_rust/lib.rs`: Rust bindings
