# Generated Runtime Interface

This document is the language-neutral contract between Plotnik-generated
matchers and their target runtime libraries. It specifies observable behavior,
not a required class layout. A runtime may use tree cursors, persistent node
handles, arrays, linked frames, or another representation as long as it obeys
the contracts below.

The bytecode VM and `plotnik-rt` are the reference implementation. See
[runtime-engine.md](runtime-engine.md) for the execution model and
[tree-navigation.md](tree-navigation.md) for the complete navigation table.

The reference ABI value is owned by `plotnik_rt::RUNTIME_ABI`. The compiler
copies that value into generated-module metadata; other runtime ecosystems
publish the ABI range they implement.

## 1. Compatibility

Dynamic-language runtimes expose an inclusive supported ABI range:

```text
RUNTIME_ABI_MIN
RUNTIME_ABI_MAX
```

A generated module records one `REQUIRED_RUNTIME_ABI` and refuses to initialize
unless it lies in that range. The current contract is ABI `2`; ABI `1` was the
first cross-language runtime contract. The ABI changes when generated code and
a runtime must change together, including changes to:

- navigation, checkpoint, or resume behavior;
- the match-journal vocabulary or payload meanings;
- the document and tree-adapter operations generated matchers call;
- limit accounting or the errors returned by safe entry points.

Adding a backend-only helper or a source-compatible convenience API does not
change the ABI. Rust gets the same compatibility check from Cargo dependency
resolution and type-checked linkage; generated Rust modules do not need the
integer gate.

An ABI mismatch is a module initialization error. It must report the required
ABI and the runtime's supported range.

## 2. Entry points and documents

Every generated definition exposes two logical operations:

```text
parse(document)   -> Result<Optional<Output>, LimitExceeded>
matches(document) -> Result<Boolean, LimitExceeded>
```

`parse` runs the matcher, then decodes its committed match journal into the
generated output type. `matches` runs the same matcher with data effects
suppressed; it does not allocate a match journal and cannot fail a decode-depth
limit.

A document binds together five things that must not drift independently:

1. the parsed tree and its root position;
2. the exact source from which that tree was parsed;
3. conversion from binding-native positions to canonical UTF-8 byte offsets;
4. the grammar/language handle used for compatibility verification;
5. the binding-native node type returned in captured output.

Rust spells this as separate `&Tree` and `&str` parameters. Output may borrow
the two independently: node fields borrow the tree and `str` fields borrow the
source. GC'd
targets should normally expose one `Document` object so its ownership and
encoding obligations cannot be separated accidentally.

The document must provide these semantic operations:

| Operation             | Meaning                                                       |
| --------------------- | ------------------------------------------------------------- |
| `root_position()`     | A fresh matcher position rooted at the tree root.             |
| `node(position)`      | The binding-native node at a position.                        |
| `source_bytes()`      | The exact source, encoded as canonical UTF-8 bytes.           |
| `byte_span(node)`     | Half-open `[start, end)` offsets into the UTF-8 source bytes. |
| `text_bytes(node)`    | The canonical source-byte slice for this node.                |
| `text(node)`          | Unicode text decoded from `text_bytes(node)`.                 |
| `source_node_count()` | Root descendant count used to resolve automatic limits.       |
| `language()`          | The language metadata queried by grammar verification.        |

Tree-sitter bindings do not agree on offset units. Canonical spans in Plotnik
are always UTF-8 bytes, even when a binding reports UTF-16 code units or Unicode
code points. The adapter owns this conversion. Predicate input, debug values,
and conformance output all use the same converted span. For example, a
web-tree-sitter document must not index a UTF-8 byte array with
`Node.startIndex`; it must translate the binding's UTF-16 position first.

The source must be the exact source used to parse the tree. A document
constructor may validate this outside-zone input. Once constructed, generated
code may treat in-range, character-boundary node spans as an invariant.

### 2.1 Node identity and liveness

Captured values contain the platform binding's public node type, not a Plotnik
wrapper. The document and runtime must therefore state how long those nodes
remain valid:

- Rust output borrows the tree;
- web-tree-sitter nodes remain valid only while their tree/document has not
  been deleted;
- Python nodes retain the objects required by py-tree-sitter;
- a Lua adapter must keep its parser/tree owner reachable for as long as a
  captured node can be used.

Disposing a document invalidates every output node borrowed from it. A runtime
must not hide that rule by copying incomplete node metadata into a lookalike
object.

## 3. Tree adapter and positions

A matcher position supports:

- save and restore;
- first-child, next-sibling, and parent movement;
- reading the current node and its field id;
- checking whether a child exists for a numeric field id.

Saved positions are opaque to generated code. A cursor binding may use a
root-relative descendant index plus an optional cursor snapshot. A binding
without cursors may retain node handles. Saving and restoring must preserve the
same logical node, including through nested calls and backtracking.

The adapter normalizes binding quirks before the matcher sees them:

- an absent field is `None`/`null`, including bindings that expose numeric `0`;
- a candidate's kind id is the public, alias-visible id, not an underlying
  grammar symbol id;
- `named`, `missing`, and `extra` are the flags for this parsed node;
- field ids and kind ids use the same numeric namespace verified at module
  initialization.

The following node class is normative:

```text
trivia(node) = !node.named || node.extra
```

An explicit match always gets a chance before the skip policy is applied. In
particular, an explicitly requested comment can match even though comments are
usually extras and therefore trivia.

### 3.1 Navigation

Generated matchers drive the navigation modes defined in
[tree-navigation.md](tree-navigation.md):

- `Epsilon` performs no tree operation;
- `Stay*` checks the current position;
- `Down*` and `Next*` move once, then search siblings according to their skip
  policy;
- `Up*(n)` validates the exit constraint at every level before ascending;
- `Childless*` checks the degenerate anchored-child case without moving.

Navigation either leaves the position ready for a candidate check and returns
its `SkipPolicy`, or fails. A failed `Up*` or `Childless*` navigation must not
leave partial movement behind.

`continue_search(policy)` is shared by initial candidate rejection and retry
after downstream failure:

| Policy   | A rejected/current candidate may be passed when |
| -------- | ----------------------------------------------- |
| `Any`    | always                                          |
| `Trivia` | it is anonymous or extra                        |
| `Extras` | it is extra                                     |
| `Exact`  | never                                           |

The same admission rule decides whether accepting a candidate creates a match
retry checkpoint. This prevents an accepted candidate from becoming an
accidental commit when a later state fails.

## 4. Engine state and backtracking

The mutable engine state consists of:

- the current position;
- call frames and current recursion depth;
- a LIFO checkpoint stack;
- the match journal and its current length;
- the current suppression depth;
- the number of open, logged scalar frames.

An instruction pointer, step counter, and resolved limit policy may live in the
generated driver's representation instead of the runtime object.

Every checkpoint snapshots all state that affects future matching:

```text
CheckpointState {
    position
    journal_watermark
    frame
    recursion_depth
    effect_depths { suppression: u32, scalar: u32 }
}
```

Restoring a checkpoint restores the position and frame, truncates the capture
trace to its watermark, and restores all depth counters. Adding mutable engine
state requires classifying it as restored or deliberately cumulative.
The two effect-control depths share one packed `u64` checkpoint field, retaining
the regression-required range above `u16` without padding every checkpoint.

There are three resume forms:

- `Successor(target)` resumes dispatch at a non-preferred successor;
- `CallRetry(target, return, field, policy)` advances to the next admissible
  candidate and re-enters the callee without repeating the call's initial
  navigation;
- `MatchRetry(state, policy)` advances past the accepted candidate and repeats
  that match's candidate checks, effects, and successor flow.

Non-preferred successors are pushed in reverse priority order so a LIFO pop tries
them in source order. A match-retry checkpoint sits below successor checkpoints
created after accepting that candidate. All successor paths at one candidate are
therefore exhausted before the search advances.

A `Call` enters a frame carrying its return state. `Return` exits that frame;
returning with no active frame accepts the entry point. Frames that no live
checkpoint can restore may be pruned, but pruning must not change behavior.

## 5. Candidate checks and predicates

A `Match` checks one candidate in this order:

1. alias-visible kind id and namedness;
2. missing-node constraint;
3. required field id;
4. absence of every negated field;
5. text predicate.

A failed check returns to the navigation search loop. On acceptance, effects
run in emitted order, then successor flow runs. Initial acceptance and
`MatchRetry` must call the same check/effect/flow implementation.

String predicates compare the exact node text:

| Operator    | Test                  |
| ----------- | --------------------- |
| `==` / `!=` | equality / inequality |
| `^=`        | starts with           |
| `$=`        | ends with             |
| `*=`        | contains              |

These operations are defined on Unicode text derived from the canonical UTF-8
source bytes.

### 5.1 Native regex execution

The runtime operation is `regex_test(id, text) → bool`. It performs an
unanchored containment search with the compiled pattern identified by `id`;
`=~` returns that boolean and `!~` negates it. Each distinct printed pattern is
compiled once when the generated module initializes, never once per candidate.

Dynamic targets use their native regex engine, but the compiler owns the
semantics. Analysis admits one portable, pure-regular subset. Normalization
then expands case folding, dot, shorthand and Unicode classes, and class set
operations against the compiler's pinned Unicode tables; removes captures;
and fixes `\b`/`\B` to ASCII word-boundary semantics. A total printer spells
that normalized HIR in each host dialect. A backend cannot reject a pattern
that passed analysis.

The printer invariant is normative: no emitted construct may consult a host's
Unicode tables, flag defaults, locale, or engine version. Text anchors always
mean whole-text start/end, and case-insensitive mode is never delegated to the
host. The haystack is node text as Unicode scalar values in the platform's
native string, obtained by a well-formed transcoding of the canonical UTF-8
source. Predicate execution does not byte-walk and TypeScript does not use
`TextEncoder` for this operation.

Rust is the representation exception, not a semantic exception: generated
Rust and bytecode use `rt::StaticDfa` compiled by regex-automata from the same
normalized HIR. Regex execution is not charged to Plotnik's state-dispatch
step counter. Engine class and worst-case running time are target properties
(Rust remains linear; some dynamic hosts backtrack), not conformance
properties; the observable boolean result is shared.

## 6. Match journal

The matcher never constructs typed values while it can still backtrack. It
records an in-memory match journal on the active path and truncates that journal
when restoring a checkpoint. The committed journal is decoded exactly once
after acceptance.

Generated runtimes implement this vocabulary:

| Journal event          | Payload and meaning                                |
| ---------------------- | -------------------------------------------------- |
| `Node`                 | Current binding-native node.                       |
| `Absent`               | One absent option/union value.                     |
| `ListOpen`             | Begin a list value.                                |
| `ArrayPush`            | Append the pending value to its backing array.     |
| `ListClose`            | Close the list and make it pending.                |
| `RecordOpen`           | Begin a record value.                              |
| `RecordSet(member)`    | Assign the pending value to a record member index. |
| `RecordClose`          | Close the record and make it pending.              |
| `VariantOpen(variant)` | Begin the selected variant.                        |
| `VariantClose`         | Close the variant and make it pending.             |
| `ScalarOpen`           | Begin one value-local source-provenance frame.     |
| `ScalarMark(node)`     | Add an explicit matched node to every open scalar. |
| `StrClose`             | Close a scalar and produce source text or null.    |
| `BoolClose(value)`     | Close a scalar and produce the supplied boolean.   |
| `NodeStr(node)`        | Produce one matched node's source text directly.   |
| `NodeBool(node)`       | Produce `true` for one matched node directly.      |
| `BoolValue(value)`     | Produce a boolean without source provenance.       |

Member and variant payloads are the indices assigned by the compiler's shared
`CaptureLayout`; they are not target-specific field ordinals. Values appear
before their closing `RecordSet`. The order of sibling `RecordSet` entries inside one
record is not stable and must not be used as declaration order.

`SuppressBegin` and `SuppressEnd` change the suppression depth but are not
journal entries. While suppression is nonzero, ordinary data events,
including scalar opens and closes, are skipped. `ScalarMark` bypasses data
suppression so an enclosing scalar still sees nodes matched inside a suppressed
definition. A mark is a no-op when no scalar frame is open. Suppression still
nests during `matches`, whose initial depth is nonzero, so `matches` allocates
no scalar events.

Inspection-span events belong to the VM/playground inspection path. Generated
production matchers reject inspection-compiled queries and do not include those
effects in the generated-runtime ABI.

Scalar effects use balanced value semantics. `ScalarOpen` starts with no range;
every mark unions the node's half-open UTF-8 byte span into the frame's hull.
`StrClose` returns `null` when the hull is absent and otherwise borrows that
slice from the source. A real `n..n` mark therefore returns `""`, not `null`.
`BoolClose` uses its boolean payload and retains the hull only as inspection
provenance; it never derives truthiness from marks.
For a scalar whose raw value is one node, `NodeStr` and `NodeBool` are the
equivalent one-entry fast path; the node also carries inspection provenance.
Production lowering uses `BoolValue(true)` for presence booleans because their
source range is not observable there; `NodeBool` and balanced boolean frames
are emitted only when inspection requests that provenance.

### 6.1 Result decoder

Typed decoders consume the committed match journal linearly. A runtime decoder provides:

- `take_absent`;
- `expect_node`, `expect_record_set`, and `expect_variant_open`;
- `expect_str` and `expect_bool` scalar leaves;
- `expect_*_open` and `expect_*_close` for lists and records;
- `expect_array_push` and close lookahead for repeated values;
- `peek_record_set`, which returns the first `RecordSet` after the balanced value beginning
  at the current position;
- `finish`, which asserts that the whole journal was consumed.

`peek_record_set` is required because a field's value precedes its member index and
different members may require different typed decoders. Implementations should
precompute matching `RecordSet` positions in one backward pass so decoding remains
linear on deeply nested output. Its balanced-value scan treats `ScalarOpen`
through either scalar close as one value, including nested scalar frames.

The decoder receives the exact source used to parse the tree. A string leaf
returns a source slice and therefore carries the source lifetime; a node leaf
carries the independent tree lifetime. Rust expresses the generic contract as
`Parse<'t, 's>`, and generated types include only the lifetimes reachable from
their output (`Q<'t>`, `Q<'s>`, `Q<'t, 's>`, or `Q`).

The compiler validates balanced journal shapes. A mismatch during decoding is an
inside-zone generated-code/runtime defect and should assert or throw as an
internal error, not be returned as invalid user input.

## 7. Limits

Safe runs resolve independent fuel and memory policies. Each policy is
`Auto`, an explicit nonnegative ceiling, or `Unbounded`.

| Resource | Automatic ceiling                | What is metered                                                 |
| -------- | -------------------------------- | --------------------------------------------------------------- |
| Fuel     | `1_000_000 + 1_024 * node_count` | Matcher dispatches; one fuel unit each today.                   |
| Memory   | `64 MiB + 256 * node_count`      | Live frames, checkpoints, capture effects, and saved positions. |

Arithmetic saturates at the target's supported maximum. A runtime may sample
memory rather than calculate it on every dispatch; the reference implementation
samples every 1,024 matcher dispatches. The error reports both ceiling and
observed usage because geometric container growth can overshoot a sampled
ceiling.

Generated typed decoding has a third limit, depth, because recursive decoders use
the platform's native stack. Its automatic ceiling is target-specific and may
use a conservative generated frame-size estimate. The iterative matcher and
the VM materializer do not have a decode-depth limit.

The portable error categories are:

```text
LimitExceeded::OutOfFuel(limit)
LimitExceeded::Memory { used, limit }
LimitExceeded::DecodeDepth(limit)
```

Limit exhaustion is an ordinary safe entry point result. Exhausted checkpoints
mean no match, not an error. An unmetered/internal entry point may assert that
limit exhaustion is impossible.

## 8. Debug value format

Conformance compares values rather than platform object layouts. Every runtime
provides a test-side serializer with this recursive JSON mapping:

- option absence and match-only output: `null`;
- source string: JSON string;
- boolean: JSON boolean;
- array: JSON array;
- struct: object keyed by generated member name;
- enum: `{ "$tag": "Variant" }`, plus `$data` when the selected variant has a
  payload;
- captured node:

```json
{
  "kind": "identifier",
  "text": "name",
  "span": [12, 16]
}
```

`kind` is the alias-visible public kind name. `text` is sliced from the
document source. `span` is the canonical half-open UTF-8 byte span, regardless
of the binding's native offset unit. This serializer is a conformance channel,
not a commitment that public output objects are JSON-shaped.

## 9. Grammar identity and verification

Generated code bakes numeric kind and field ids. A parser built from another
grammar revision can renumber them while still returning a valid tree, so every
entry point verifies the tree's language before matching.

The generated module records this provenance:

```text
GrammarIdentity {
    name
    grammar_sha256
    source
}
```

- `name` is the grammar's declared name;
- `grammar_sha256` is lowercase SHA-256 of the exact `grammar.json` bytes used
  for binding;
- `source` is a diagnostic label, such as a registry language/version or the
  path passed to `--grammar`.

The identity appears in generated header comments and constants. It is
diagnostic provenance, not something most tree-sitter bindings can verify from
a live language object.

Enforcement is a subset check over the ids the generated matcher actually uses:

```text
EXPECTED_KINDS  = [(id, name, named), ...]
EXPECTED_FIELDS = [(id, name), ...]
```

For each expected kind, the live language must return the same public name and
namedness for that id. For each expected field, it must return the same name.
Unused grammar entries may differ. This tolerance is deliberate: two grammar
revisions that preserve every id observed by a query are compatible with that
generated module.

A mismatch is a grammar-skew error and includes the expected id/name, the live
value, and the recorded `GrammarIdentity`. The remedy is to regenerate against
the `grammar.json` belonging to the parser package used in production. A
`generate --grammar <path>` flow must therefore accept that exact artifact;
registry language names are a convenience, not a substitute for production
provenance.

Conformance corpus cases record the same identity. Runtime runners pin their
platform grammar dependency so a verification failure identifies an explicit
dependency skew.

## 10. Conformance requirements

A target is conforming when its runner executes the shared corpus and agrees
with the VM oracle on:

- match/no-match and portable limit category;
- the committed match journal, including layout indices, captured-node byte
  spans, scalar marks, and scalar close values;
- the debug value after typed decoding;
- grammar-skew and runtime-ABI failures.

The corpus must cover every navigation and resume mode, field and missing-node
checks, all predicate operators, suppression, recursive calls, source-order
alternative priority, journal truncation after backtracking, nested decode shapes, automatic
and explicit limits, scalar item boundaries and zero-byte ranges, and non-ASCII
source before captured nodes. Regex cases
exercise every dialect printer's semantic traps and are the tripwire for
normalization or host-spelling drift.
