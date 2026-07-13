# Tree Navigation

How the VM navigates tree-sitter syntax trees. This covers API choice, search loop mechanics, and anchor lowering. For execution semantics, see [runtime-engine.md](runtime-engine.md). For instruction encoding, see [06-transitions.md](binary-format/06-transitions.md).

## TreeCursor API

The VM uses `TreeCursor` exclusively, never the `Node` API for traversal.

```rust
struct VM<'t> {
    cursor: TreeCursor<'t>,          // created at tree root, never reset
    ip: CodeAddr,                    // current instruction address
    frames: Vec<Frame>,              // call stack
    journal: MatchJournal<'t>,       // rollbackable match journal
    suppress_depth: u64,             // suppressive capture depth
}

struct Checkpoint {
    descendant_index: u32,             // cursor position (4 bytes)
    journal_watermark: usize,          // match journal length
    frame_index: Option<u32>,          // call stack state
    ip: CodeAddr,                      // branch target, or the owning Call/Match
    resume: Resume,                    // Branch | Call(CallResume) | Match
}
```

A `Branch` checkpoint resumes dispatch at `ip`. A `Call` resume carries everything needed to retry a `Call` at a later sibling — callee entry, return address, field constraint, and skip policy — so backtracking advances the cursor and re-enters the callee without re-running the `Call`'s navigation. A `Match` resume marks the accepted candidate of an in-pattern sibling search: backtracking advances past it (per the skip policy re-derived from the instruction at `ip`) and re-runs the same match's candidate search from there. Keeping resume state on the checkpoint, rather than in ambient VM state, is what gives every sibling search — Call-driven or in-pattern — the same backtracking power (see [Call Navigation](#call-navigation)).

**Critical constraint**: The cursor must be created at the tree root and never call `reset()`. The `descendant_index` is relative to the cursor's root — `reset(node)` would invalidate all checkpoints.

### Why TreeCursor

| Operation             | TreeCursor | Node        |
| --------------------- | ---------- | ----------- |
| `goto_first_child()`  | O(1)       | —           |
| `goto_next_sibling()` | O(1)       | O(siblings) |
| `goto_parent()`       | O(1)       | O(1)        |
| `descendant_index()`  | O(1)       | —           |
| `goto_descendant()`   | O(depth)   | —           |

The `Node` API's `next_sibling()` is O(siblings) — unacceptable for repeated backtracking. TreeCursor provides O(1) sibling traversal and 4-byte checkpoints via `descendant_index`.

- Checkpoint save: O(1)
- Checkpoint restore: O(depth) — cold path only

## Nav Encoding

`Nav` is a single byte encoding movement and skip policy. See [06-transitions.md § 3.1](binary-format/06-transitions.md) for bit layout.

| Nav                   | Dump Symbol | Movement                                |
| --------------------- | ----------- | --------------------------------------- |
| `Epsilon`             | `-ε-`       | Pure control flow                       |
| `Stay`                | (space)     | No movement                             |
| `StayExact`           | `!`         | No movement, exact match only           |
| `Down`                | `└‣─`       | First child, skip any                   |
| `DownSkip`            | `└•─`       | First child, skip trivia only           |
| `DownSkipExtras`      | `└◦─`       | First child, skip extras only           |
| `DownExact`           | `└─!`       | First child, exact                      |
| `Next`                | `─‣─`       | Next sibling, skip any                  |
| `NextSkip`            | `─•─`       | Next sibling, skip trivia               |
| `NextSkipExtras`      | `─◦─`       | Next sibling, skip extras               |
| `NextExact`           | `──!`       | Next sibling, exact                     |
| `Up(1)`               | `─‣┘`       | Ascend 1 level                          |
| `Up(2)`               | `─‣┘²`      | Ascend 2 levels                         |
| `UpSkipTrivia(2)`     | `─•┘²`      | Ascend 2, last non-trivia on each level |
| `UpSkipExtras(2)`     | `─◦┘²`      | Ascend 2, last non-extra on each level  |
| `UpExact(2)`          | `!─┘²`      | Ascend 2, last child on each level      |
| `ChildlessSkipTrivia` | `└•┘`       | Assert only trivia children, no move    |
| `ChildlessSkipExtras` | `└◦┘`       | Assert only extra children, no move     |
| `ChildlessExact`      | `└!┘`       | Assert no children at all, no move      |

## Search Loop

Navigation and matching are intertwined. The `Nav` mode determines initial movement and skip policy.

### Algorithm

```
1. MOVE    Execute nav (goto_first_child, goto_next_sibling, etc.)
2. SEARCH  Loop: try match, on fail apply skip policy
3. EFFECTS On success: execute the match's effects list in order
```

For `Up*` variants, step 2 becomes: for each of the n levels, validate the exit
constraint on the node being left, then ascend one level. The constraint is
checked at **every** level, which is what lets same-mode `Up*` instructions
compose: `Up*(a)` followed by `Up*(b)` is exactly `Up*(a+b)`.

### Skip Policy

Each mode defines what happens when a match fails:

**Down/Next variants** (search loop):

| Policy glyph | On Match Fail                               |
| ------------ | ------------------------------------------- |
| `‣` (any)    | Advance and retry until exhausted           |
| `•` (trivia) | If current is non-trivia → fail; else retry |
| `◦` (extras) | If current is non-extra → fail; else retry  |
| `!` (exact)  | Fail immediately                            |

The same policy governs match **acceptance**: a candidate accepted by a
non-exact `Down*`/`Next*` search leaves a match-retry checkpoint whenever the
policy would admit that node as a skipped gap filler (any node under `‣`, only
trivia/extras under `•`/`◦`). A later failure — even deep inside the accepted
candidate's subtree — then resumes the search at the next admissible sibling
instead of silently committing. Steps internal to a compiled retry loop
(`emit_position_search` wrappers) are emitted with exact navs precisely to opt
out of this: every search has exactly one retry owner, either the engine's
in-instruction search or the NFA loop, never both.

**Up variants** (exit validation):

| Mode              | Constraint (checked at each of the n levels)              |
| ----------------- | --------------------------------------------------------- |
| `Up(n)`           | None — just ascend n levels                               |
| `UpSkipTrivia(n)` | Each node left must be its parent's last non-trivia child |
| `UpSkipExtras(n)` | Each node left must be its parent's last non-extra child  |
| `UpExact(n)`      | Each node left must be its parent's last child            |

**Childless variants** (zero-width anchors):

When a node's whole child list matches zero-width, the cursor never descends,
so no `Down*` entry carries a leading anchor's first-child check and no `Up*`
ascent carries a trailing anchor's lastness check. `Childless*` asserts the
degenerate form of either: the node has no children the anchor's skip policy
would reject. When both anchors demand one, the tighter check alone is
emitted (the admitted-child sets nest). A body of anchors alone (`(node .)`)
always takes this path — there is nothing to descend into, so the childless
check is the entire compiled body. The cursor does not move; failure
backtracks like any nav failure.

| Mode                  | Constraint              |
| --------------------- | ----------------------- |
| `ChildlessSkipTrivia` | Every child is trivia   |
| `ChildlessSkipExtras` | Every child is an extra |
| `ChildlessExact`      | No children at all      |

### Example: `(foo (bar))` vs `(foo (foo) (foo) (bar))`

With `Nav::Down` (skip any):

1. `goto_first_child` → cursor at first `foo`
2. Try match `bar` → fail
3. Skip policy: any → `goto_next_sibling`
4. Try match `bar` → fail
5. `goto_next_sibling` → cursor at `bar`
6. Try match `bar` → success

With `Nav::DownExact`:

1. `goto_first_child` → cursor at first `foo`
2. Try match `bar` → fail
3. Skip policy: exact → fail immediately

## Trivia

**Trivia** = anonymous nodes + nodes tree-sitter marks as `extra` for that specific parse instance.

The `*Skip` modes skip trivia automatically but fail if a non-trivia node must be skipped.

The VM reads the parser's `Node::is_extra()` bit at runtime; there is no bytecode trivia table.

**Skip invariant**: A node is never skipped if its kind matches the target. This ensures `(comment)` explicitly in a query still matches, even though comments are typically trivia.

## Anchor Lowering

Anchors compile to `Nav` variants by spelling and operand type:

| Position              | Named-only `.`    | Anonymous-involved `.` | `.!` exact anchor |
| --------------------- | ----------------- | ---------------------- | ----------------- |
| Start of children     | `DownSkip`        | `DownSkipExtras`       | `DownExact`       |
| Between sibling items | `NextSkip`        | `NextSkipExtras`       | `NextExact`       |
| End of children       | `UpSkipTrivia(1)` | `UpSkipExtras(1)`      | `UpExact(1)`      |

`.` skips extras in all cases. It also skips anonymous nodes when both sides are named. `.!` allows no child node in the constrained gap.

Bare `_` is an anonymous wildcard, so `(a) . _` uses extras-only navigation. `(_)` is a named wildcard, so `(a) . (_)` uses trivia-skipping navigation.

An anchor next to an alternation is classified per branch on both sides. Before: `(a) . [(b) ","]` uses `NextSkip` for `(b)` and `NextSkipExtras` for `","`. After a named follower: `[(b) ","] . (a)` emits two copies of the follower's entry instruction — `NextSkip` and `NextSkipExtras`, sharing successors — and routes the named `(b)` path to the `NextSkip` copy and the `","` path to the `NextSkipExtras` copy. Only one copy runs per match path, so duplicated capture effects fire exactly once.

The split fires when the alternation's exit is the follower's own single `Match` on a named node _and_ the matched branch ends on a named node. This covers the common forms — including inline-effect captures whose effects ride the branch instructions rather than wrapping the exit: a scalar `[(b) ","] @x . (a)` and an uncaptured enum `[A: (b) B: ","] . (a)` both split. It stays conservative (extras-only for every branch) — correct but not yet optimal — in these cases, pending follow-up:

- The follower is itself anonymous (`. ","`) or `_`: both-sides-named never holds, so extras-only is in fact correct.
- The follower is a ref (`. (Rule)`, a `Call`) or scope-wrapped (`. (a (b) @c) @x`, an epsilon entry): no single named `Match` to clone.
- The alternation's value is materialized through a trailing effect epsilon rather than inline — a record/list scope capture, or a variant alternation captured by name (`[A: (b) B: ","] @t . (a)`) — so its exit is that epsilon (the `RecordSet`/`RecordClose`), not the follower's `Match`.
- A branch is quantified (`[(b)? ","] . (a)`): its zero-match path leaves no named node on the anchor's left, so the upgrade is unsound. The whole branch stays extras-only.
- A branch is a sequence ending in a named node (`[{(b) "," (c)} ";"] . (a)`): branch namedness is classified over the whole branch (matching the before-anchor classifier), so a branch containing any anonymous token is treated as anonymous even when its tail is named. Conservative, not a wrong match. A trailing-position classifier would lift this.

### Compilation Examples

Using dump format from [08-dump-format.md](binary-format/08-dump-format.md):

> These examples show the _logical_ anchor lowering — the nav mode each anchor produces. An item that must be located among its siblings (an unpinned first item, or any item reached past a skipping anchor) additionally compiles to a small search wrapper: a branch into the candidate plus an advance-and-retry edge, converging on the anchored continuation. The wrapper is uniform across alternations, anchors, and quantifiers (one `emit_position_search` combinator). Run `dump` for the exact instructions; the rows below omit the wrapper for readability.

**Simple**: `(function (identifier) @name)`

```
  01       (function)                           02
  02  └‣─  (identifier) [Node Set(M0)]          03
  03  ─‣┘                                       ◼
```

**First child anchor**: `(function . (identifier))`

```
  01       (function)                           02
  02  └•─  (identifier)                         03
  03  ─‣┘                                       ◼
```

**Exact first-child anchor**: `(function .! (identifier))`

```
  01       (function)                           02
  02  └─!  (identifier)                         03
  03  ─‣┘                                       ◼
```

**Last child anchor**: `(function (identifier) .)`

```
  01       (function)                           02
  02  └‣─  (identifier)                         03
  03  ─•┘                                       ◼
```

**Exact last-child anchor**: `(function (identifier) .!)`

```
  01       (function)                           02
  02  └‣─  (identifier)                         03
  03  !─┘                                       ◼
```

**Adjacent siblings**: `(block (a) . (b))`

```
  01       (block)                              02
  02  └‣─  (a)                                  03
  03  ─•─  (b)                                  04
  04  ─‣┘                                       ◼
```

**Soft adjacency with an anonymous operand**: `(call (identifier) . "(")`

```
  01       (call)                               02
  02  └‣─  (identifier)                         03
  03  ─◦─  "("                                  04
  04  ─‣┘                                       ◼
```

Anonymous operands make `.` skip extras only. Comments can appear between the operands; other anonymous tokens cannot. Use `.!` when no syntax-tree node may intervene; it does not require adjacent source bytes.

**Exact adjacency**: `(call (identifier) .! "(")`

```
  01       (call)                               02
  02  └‣─  (identifier)                         03
  03  ──!  "("                                  04
  04  ─‣┘                                       ◼
```

**Deep nesting**: `(a (b (c (d))))`

```
  01       (a)                                  02
  02  └‣─  (b)                                  03
  03  └‣─  (c)                                  04
  04  └‣─  (d)                                  05
  05  ─‣┘³                                      ◼
```

Multi-level ascent coalesces: the compiler merges consecutive effectless `Up*` steps of the same mode into one instruction, capped at the level field's encoding limit (31). This holds for the constraint-carrying modes too — `Up*` composes, so the merged instruction re-checks the constraint at every level it ascends (see [Search Loop](#search-loop)). A run deeper than the cap splits into several adjacent instructions whose per-level checks partition the levels with no gap.

**Mixed anchors**: `(a (b) . (c) .)`

```
  01       (a)                                  02
  02  └‣─  (b)                                  03
  03  ─•─  (c)                                  04
  04  ─•┘                                       ◼
```

The `.` before `(c)` → `NextSkip`; the `.` after `(c)` → `UpSkipTrivia`.

**Intermediate anchors**: `(array {(object (pair) .) (number)})`

```
  02       (array)                              03
  03  └‣─  (object)                             04
  04  └‣─  (pair)                               05
  05  ─•┘                                       06
  06  ─‣─  (number)                             07
  07  ─‣┘                                       ◼
```

The `.` after `(pair)` produces `─•┘` (exit object, pair must be last non-trivia). Then `─‣─` finds sibling `(number)`, and `─‣┘` exits array. Steps 05 and 07 are **not** adjacent — the `─‣─` sibling match sits between them — so they are never coalesced. (Were they adjacent, merging into `UpSkipTrivia(2)` would be sound: `Up*` composes, checking each level in turn.)

## Field Handling

Field constraints are checked during the match attempt within the search loop. The `Match` instruction stores `node_field` as an optional constraint:

```rust
pub struct Match {
    pub nav: Nav,
    pub node_kind: NodeKindConstraint,   // Any = wildcard
    pub node_field: Option<NonZeroU16>,  // None = no constraint
    pub neg_fields: Vec<u16>,            // must NOT be present
    // ...
}
```

Inside the search loop:

```rust
// Before node kind check:
if let Some(required) = pattern.node_field {
    if cursor.field_id() != Some(required) {
        // Field mismatch → apply skip policy
        continue;
    }
}
```

**Negated fields** are checked after node kind matches:

```rust
// After node kind matches:
for &fid in pattern.neg_fields {
    if node.child_by_field_id(fid).is_some() {
        // Negated field present → apply skip policy
        continue;
    }
}
```

Both constraints participate in the skip policy — a mismatch triggers retry (for `*`), fail-if-non-trivia (for `~`), or immediate fail (for `.`).

## Call Navigation

`Call` instructions handle navigation for recursive patterns. The caller provides nav and field constraint; the callee's first `Match` checks node kind:

```rust
pub struct Call {
    pub nav: Nav,
    pub node_field: Option<NonZeroU16>,
    pub next: SuccessorAddr,      // return address
    pub target: SuccessorAddr,    // callee entry
}
```

For `field: (Ref)` patterns, this allows checking field and type on the same node without extra instructions.

### Sibling search and resume

A `Call` navigates to its first candidate the same way a `Match` does. When the nav is a searching one (skip policy `Any`/`Trivia`/`Extras`, i.e. not `Exact`/`Stay`), the callee may need to be retried at later siblings if it fails — the candidate is not fixed. `exec_call` handles this by pushing a `call_resume` checkpoint (see [Checkpoint](#treecursor-api)) _before_ entering the callee. On backtrack, that checkpoint advances the cursor to the next candidate (honoring the skip invariant and any field constraint) and re-enters the callee, looping until the siblings are exhausted. `Exact`/`Stay` calls have a single fixed candidate and push no retry checkpoint.

This is the same forward-search-with-backtracking that in-pattern anchors and quantifiers use; the resume state lives on the checkpoint so there is exactly one notion of "advance to the next sibling and try again," shared by `Call` retry and pattern search alike.

### Zero-width returns

A `Call`'s return address carries the navigation the follower needs _after the callee consumed its candidate_ — there is no return path that says "nothing was consumed, stay put." So references to _nullable_ definitions (bodies that can match zero nodes, e.g. `A = (x)?`) are not compiled as calls at all: the body is inlined at the reference site, where the ordinary skip-path machinery (checkpoint cursor restore, split follower navigation) applies exactly as if the body were written inline.

The one exception is a nullable reference back into a definition currently being compiled (`A = (x (A) (y))?`). It becomes a real call guarded by a zero-width bypass branch that is tried first. The call targets a consuming-only body, so the bypass covers the empty case and every actual call path consumes input.
