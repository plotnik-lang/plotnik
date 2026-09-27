# Effects and result construction

- [Design](#design)
- [Implementation](#implementation)
  - [Encoding](#encoding)
  - [Values and frames](#values-and-frames)
  - [Scalar values and provenance](#scalar-values-and-provenance)
  - [Suppression](#suppression)
  - [Inspection brackets](#inspection-brackets)
  - [Valid effect sequences](#valid-effect-sequences)
    - [Reader limits](#reader-limits)

## Design

An effect is an action a `Match` instruction performs after its node tests pass, such as capturing a node, adding a result field, or recording an inspection event. Each instruction stores its effects in order. A later failure can still abandon that path, so its values and inspection observations remain subject to backtracking.

Result construction uses two kinds of state. The pending value is a completed value waiting to be attached or returned. Open frames track values under construction, such as a list waiting for more items or a record waiting for more fields. Closing a container makes it the pending value, ready to become part of an enclosing container.

For example, suppose the cursor is on an identifier whose text is `"alpha"`, and member 7 describes a text field named `name`. These effects build a one-field record:

| Effect         | Open record         | Pending value       |
| -------------- | ------------------- | ------------------- |
| `RecordOpen`   | `{}`                | Empty               |
| `NodeText`     | `{}`                | `"alpha"`           |
| `RecordSet(7)` | `{ name: "alpha" }` | Empty               |
| `RecordClose`  | None                | `{ name: "alpha" }` |

`NodeText` produces a value. `RecordSet` attaches that value and clears pending, making room for another field. `RecordClose` then produces the finished record. The distinction between empty pending and an absent value matters: `Absent` also produces a value that can be attached, such as an absent list item or record field. Even absence has to wait its turn.

Frames nest when values nest. A list inside a record becomes one pending list when it closes. A following `RecordSet` can attach the whole list as one field. Member operands identify entries in the global type-member table, so member 7 above is not the seventh field of the open record.

A text or boolean value can retain a document range showing where it came from, its provenance. A `Scalar` frame collects this range as matching proceeds. `ScalarMark` contributes the current matched node's range to every open scalar frame. The resulting range spans from the earliest marked start to the latest marked end. For the source `alpha + beta`, marks on the two identifiers cover the text between them too:

```text
source             alpha + beta
marked nodes       [----)  [---)
scalar range       [-----------)
```

Closing that frame with `TextClose` produces `"alpha + beta"`. Closing it with `BoolClose(1)` produces `true`, while retaining the same source range. The marks explain where a scalar came from. They do not determine a boolean's truth value.

A span identifies a location in the query, such as a capture or node pattern. Paired effects mark when execution enters and leaves that construct. Each visit is an occurrence of the span, so a repeated pattern can have several occurrences. `SpanStart` and `SpanStartAt` open an occurrence, and `SpanEnd` closes it. `SpanStartAt` also records the current node as its initial matched range.

These inspection brackets nest independently of value frames. A span can surround several values, and a value can contain several span occurrences:

```text
value frames                 inspection brackets
RecordOpen                   SpanStart(2)
| ScalarOpen                 | SpanStartAt(3)
| | ScalarMark               | |
| TextClose                  | SpanEnd(3)
| RecordSet(7)               SpanEnd(2)
RecordClose
```

A reused body sometimes needs to match without adding its own value to the caller's result. Suppression disables its output effects, including frame opens and closes. Matching still proceeds normally. `ScalarMark` and inspection brackets bypass suppression, so a surrounding capture can retain provenance from a suppressed body.

Backtracking restores the pending value, open frames, suppression scopes, scalar marks, and inspection brackets together. A failed alternative cannot leave a field, item, or source range in the accepted result.

## Implementation

An extended `Match` stores its effects in execution order. Its navigation and constraints succeed before any of those effects take place. Effects on a failed path are discarded back to the restored choice point. Only the accepted path contributes to the final value and inspection output.

### Encoding

An effect is a `u16`:

```text
bits     15..10         9..0
       +--------------+--------------------+
       | opcode       | payload            |
       +--------------+--------------------+
```

Member operands are absolute indices in the global [TypeMembers table](03-types.md), not offsets within a record or variant. Span operands index the [Spans section](05-spans.md).

Output effects obey suppression. Control effects open and close suppression scopes, and bypass effects remain active inside those scopes. The cursor-reading effects also require a nonempty [match path](02-instructions.md#empty-paths).

| Opcode | Name            | Payload         | Suppression | Reads cursor |
| ------ | --------------- | --------------- | ----------- | ------------ |
| 0      | `Node`          | 0               | Output      | Yes          |
| 1      | `ListOpen`      | 0               | Output      | No           |
| 2      | `ArrayPush`     | 0               | Output      | No           |
| 3      | `ListClose`     | 0               | Output      | No           |
| 4      | `RecordOpen`    | 0               | Output      | No           |
| 5      | `RecordSet`     | Member index    | Output      | No           |
| 6      | `RecordClose`   | 0               | Output      | No           |
| 7      | `VariantOpen`   | Member index    | Output      | No           |
| 8      | `VariantClose`  | 0               | Output      | No           |
| 9      | `Absent`        | 0               | Output      | No           |
| 10     | `SuppressBegin` | 0               | Control     | No           |
| 11     | `SuppressEnd`   | 0               | Control     | No           |
| 12     | `SpanStartAt`   | Span index      | Bypass      | Yes          |
| 13     | `SpanStart`     | Span index      | Bypass      | No           |
| 14     | `SpanEnd`       | Span index      | Bypass      | No           |
| 15     | `ScalarOpen`    | 0               | Output      | No           |
| 16     | `ScalarMark`    | 0               | Bypass      | Yes          |
| 17     | `TextClose`     | 0               | Output      | No           |
| 18     | `BoolClose`     | Boolean, 0 or 1 | Output      | No           |
| 19     | `NodeText`      | 0               | Output      | Yes          |
| 20     | `NodeBool`      | 0               | Output      | Yes          |
| 21     | `BoolValue`     | Boolean, 0 or 1 | Output      | No           |

Opcodes 22–63 are invalid. Member operands are less than `type_members_count`, and span operands are less than `spans_count`. Effects shown with payload zero reject any nonzero payload. `BoolClose` and `BoolValue` accept only zero or one. Suppressed and unreachable effects have the same encoding restrictions.

### Values and frames

Result construction uses a pending [value](03-types.md), initially empty, and nested `List`, `Record`, `Variant`, and `Scalar` frames. The current frame is the innermost open frame. A pending absent value is a produced value, distinct from no pending value.

- `Node` produces the current node, including its kind name, exact source text, and source byte range. `Absent` produces an absent value.
- `ListOpen` opens an empty `List` frame. `ArrayPush` appends the pending value to the current `List` frame and clears pending. `ListClose` closes that frame and produces its list.
- `RecordOpen` opens an empty `Record` frame. `RecordSet(m)` appends the pending value under member `m`'s name to the current `Record` or `Variant` frame and clears pending. `RecordClose` closes the current `Record` frame and produces its record.
- `VariantOpen(m)` opens a `Variant` frame whose case name and payload type are member `m`'s name and type. `VariantClose` closes that frame and produces the case with its payload, if any.

List items and record fields retain attachment order. `RecordSet` uses a global member's name. It does not select a slot relative to the enclosing frame or replace an earlier attachment with that name. The reader does not reject repeated names, prove membership in an enclosing declared record, or check the value against the member's declared type. The completed value still has to match the declared result type.

A variant payload has exactly one of these forms. The kind is the case member's direct type kind, and fields are direct attachments to its `Variant` frame.

| Kind      | Pending | Fields      | Payload            |
| --------- | ------- | ----------- | ------------------ |
| `NoValue` | Empty   | None        | None               |
| Any other | Present | None        | Pending value      |
| Any other | Empty   | One or more | `Record` of fields |

All other combinations are invalid. Pending absent still counts as a payload, and a case cannot combine a pending value with direct fields. An empty record payload requires a produced empty `Record` value. No alias resolution participates in the `NoValue` test. `VariantOpen` requires an existing type for its member, even when that member lies outside all composite member runs.

If the completed accepted execution has no pending value, its result is absent. Otherwise its result is the pending value. The selected entry's [boundary effects](02-instructions.md) are part of this construction.

### Scalar values and provenance

`ScalarOpen` opens an unmarked `Scalar` frame. A `ScalarMark` records the current explicit matched node in every open `Scalar` frame whose opener was not suppressed, including frames enclosing intervening `List`, `Record`, or `Variant` frames. With no such `Scalar` frame it has no effect. Marks do not produce pending values.

The frame's provenance extends from the earliest marked start offset to the latest marked end offset:

```text
marked nodes       [----)       [------)
scalar provenance  [-------------------)
```

An unmarked frame has no provenance. Source between marked nodes belongs to the bounding interval even when no node there was marked. A real zero-width node contributes a range with equal start and end offsets, which is distinct from no provenance. Marks from failed paths do not contribute.

`TextClose` and `BoolClose` close the current `Scalar` frame and retain its optional range as provenance:

- `TextClose` produces the source slice of that range, or absent when the frame is unmarked. A zero-width marked range produces `""`.
- `BoolClose(b)` produces boolean `b`, independent of marks. Text content, range width, and mark presence do not determine its value.

Direct scalar effects require no `Scalar` frame:

- `NodeText` produces the exact source text of the current node, with that node's range as provenance.
- `NodeBool` produces `true`, with the current node's range as provenance.
- `BoolValue(b)` produces boolean `b` without provenance.

Source slices use byte offsets into the UTF-8 source associated with the syntax tree, without normalization or trimming.

Only explicit `ScalarMark` effects contribute to `Scalar` frames. `Node` captures, direct node scalars, and inspection starts do not implicitly mark them. Nested `Scalar` frames receive the same marks while both are open. An inner close does not remove those marks from its enclosing frame.

The placement of `ScalarOpen` and its close determines which marks belong to a scalar. Moving either across a node match can change both text and provenance. Direct node scalar forms and framed scalar forms are both valid representations. `BoolValue` is suitable when the boolean carries no source provenance.

### Suppression

Suppression is a nested depth, initially zero for result-producing execution. `SuppressBegin` increases it by one. `SuppressEnd` decreases it by one. A close without an open scope is invalid. Every body balances its own suppression scopes before returning.

At positive suppression depth:

- Output effects make no result-construction change, including no frame open or close.
- Control effects still change suppression depth.
- Bypass effects still produce span occurrences and scalar marks for open `Scalar` frames.

Suppressed node output effects do not snapshot the cursor. `ScalarMark` bypasses suppression only for `Scalar` frames that were actually opened. A suppressed `ScalarOpen` creates no frame. An enclosing `Scalar` can retain provenance from a suppressed nested value without constructing that nested value.

Suppression does not change matching, navigation, predicates, successor preference, or call/return selection. Its depth and open `Scalar` state are restored with the rest of a failed path. A call under suppression retains its caller's pending value and frames while its output effects remain suppressed. Its span effects and applicable scalar marks still contribute on success.

A match-only execution may impose suppression for the entire run. Encoded scopes remain balanced above that execution mode's suppression. This changes neither the stored encoding nor the outcome of matching.

### Inspection brackets

`SpanStartAt(i)` opens an occurrence of span `i` with a snapshot of the current node. `SpanStart(i)` opens an occurrence without a node snapshot. `SpanEnd(i)` closes the innermost occurrence and names that same span ID. Occurrences may nest, including repeated occurrences of the same ID. Underflow, mismatched IDs, and unclosed occurrences are invalid on an accepted path.

Value frames and inspection brackets nest independently.

Span effects bypass suppression and leave pending values unchanged. An occurrence without matched provenance has no range. Span occurrences from failed paths are discarded with their effects.

### Valid effect sequences

The reader checks every encoded alternative in each reachable body. Node and predicate conditions do not exclude a path from this check. Result-construction rules apply when output is unsuppressed. Suppression and inspection brackets are checked regardless of output suppression. The [instruction rules](02-instructions.md#empty-paths) separately prohibit cursor-reading effects on empty paths, even when suppression would prevent their runtime read.

The pending-value requirements are:

- `Node`, `Absent`, `NodeText`, `NodeBool`, and `BoolValue` require pending empty and leave it present.
- Frame opens require pending empty and leave it empty.
- `ArrayPush` requires pending present and a current `List` frame. `RecordSet` requires pending present and a current `Record` or `Variant` frame. Both attachments leave pending empty.
- `ListClose`, `RecordClose`, `TextClose`, and `BoolClose` require pending empty and the matching current frame. They leave pending present.
- `VariantClose` requires a current `Variant` frame with a valid payload form above and leaves pending present.
- An unsuppressed call requires pending empty. Any callee attachments require a compatible caller frame. The call's returned pending state is the callee's.

Frames close in reverse opening order, with their corresponding close kind. A body cannot close a frame supplied by its caller. It may attach to a caller `List` with `ArrayPush`, or to a caller `Record`/`Variant` with `RecordSet`, when no local frame is open. Such attachments impose the same frame-kind requirement at every unsuppressed call site. Transitive calls retain that requirement.

A `Return` leaves no local frame, suppression scope, or span occurrence open. All successful `Return` paths of one body agree on whether they return a pending value, across all of its ports. A return may leave data attached to an enclosing caller frame. Calls under suppression preserve the caller's pending state instead of receiving an output value.

For entry execution, a `Record` boundary supplies the enclosing `Record` frame. `Node` and `Passthrough` boundaries supply no frame, so their bodies cannot require caller-frame attachments. `Node` and `Record` boundaries require no pending value from the body, because the boundary produces the final value. `Passthrough` may produce a value or leave none. These conditions do not establish compatibility with the entry's declared type.

A terminal `Match` accepts entry execution with local construction frames and brackets balanced. A terminal `Match` is invalid in a called body reachable from an entry.

#### Reader limits

The reference reader checks result-effect paths from entries and their reachable calls. Each reachable body is checked independently from empty pending, no local frames or span occurrences, and suppression depth zero. This applies even when all its callers suppress output. Attachments to a caller frame still impose the frame-kind requirements described above.

Encodings and operand bounds remain checked across the entire instruction stream. The reader rejects cycles with unbounded growth of open frames, suppression scopes, or span occurrences. It may also reject an otherwise valid graph when a body analysis exceeds 262,144 distinct combinations of instruction address, frame state, suppression depth, span state, and pending-value state. The bound is a reader limit, not an additional rule for the stored encoding.

If an unsuppressed callee may attach a field to its caller's previously unwritten `Variant`, the reader requires valid continuations for both a `Variant` with direct fields and one still without direct fields. This conservative rule applies even when the callee always writes a field.

For example, suppose a case requires a payload, and its callee always writes a field and returns no pending value. If that field is the case's only payload, a following `VariantClose` is rejected because the reader also checks the continuation with no fields. Inlining the same field-writing effects can pass.

The reader checks entry-boundary pending compatibility from the body's `Return` paths. A root-only terminal branch is checked for balanced frames and brackets but does not contribute to that pending-state summary. The entry's pending-value requirement still applies to terminal branches.

These checks establish balanced construction and the direct `NoValue` variant condition. They do not prove that the result matches its declared type, that fields belong to the enclosing record, or that their names and order match the declaration. A writer still has to satisfy the declared [type](03-types.md).
