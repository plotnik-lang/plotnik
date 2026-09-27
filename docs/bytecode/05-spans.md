# Inspection spans

- [Design](#design)
- [Implementation](#implementation)
  - [Entry](#entry)
  - [Kinds](#kinds)
  - [Bindings](#bindings)
  - [Inspection occurrences](#inspection-occurrences)
  - [Capacity](#capacity)
  - [Validation](#validation)

## Design

Inspection connects a part of the query to what happened when it ran. A span entry identifies the query construct and its location in the query source. Span effects mark when execution enters and leaves that construct, creating an occurrence for an inspector to display.

One query location can execute several times. For example, suppose a repeated `(identifier)` pattern is reached for both identifiers in the document `x + x`. The pattern's query location stays the same, while its two occurrences cover different document text. Here the pattern begins at byte 20 in query source 0:

```text
Query fragment at byte 20: (identifier)

Span entry for the pattern
  source_id 0, start 20, end 32
    |
    +-- occurrence 1 --> document bytes 0..1 --> first x
    |
    +-- occurrence 2 --> document bytes 4..5 --> second x

Matched document: x + x
```

The query range covers `(identifier)`. Each document range covers an `x`. Both use byte offsets, with the end just past the last included byte, but they refer to different texts. Only the query range is stored in `Spans`. Document ranges come from execution, and an occurrence can have none if it gathers no matched location.

The inspector needs the original query sources to display their text. The bytecode stores only their locations. Repetition and recursion can create several occurrences of one span entry. Their nesting follows the span effects that executed, not the order of entries or the containment of their query ranges.

A span can also identify a result type, field, or variant case. This binding connects query syntax to the result's declared structure. It does not encode a particular list index or result path, which depends on execution. Some entries provide only source information, and some, such as type annotations written as `:: text`, do not open an execution occurrence at all.

Inspection has a fixed capacity. When all detail does not fit, the compiler keeps whole groups of more useful constructs, starting with definitions and captures. Omitting detail changes what the inspector can show without changing matches or result values.

## Implementation

`Spans` contains `spans_count` entries and can be empty. A span ID is the zero-based index of an entry. [Span effects](04-effects.md#inspection-brackets) carry this ID in their 10-bit payload, so the table has at most 1024 entries, with IDs 0 through 1023.

### Entry

Each entry occupies 16 bytes. The first fields identify the query source and construct. The last two fields optionally bind it to a result type or member.

```text
0             2      3       4         8         12        14        16
+-------------+------+-------+---------+---------+---------+---------+
| source_id   | kind | flags | start   | end     | type_id | member  |
+-------------+------+-------+---------+---------+---------+---------+
     u16        u8     u8       u32       u32       u16       u16
```

`source_id` selects an external query source. The bytecode stores neither the sources, their names, nor their count. The compiler numbers sources in their insertion order, starting at zero. Every 16-bit value is an ordinary source index, including `0xffff`.

`start` and `end` are UTF-8 byte offsets in that source. The range includes `start` and excludes `end`. Equal offsets describe an empty range. Writers supply ranges within the source and on UTF-8 boundaries. The reader checks only that `start` is no greater than `end`. It cannot check source existence, source length, or character boundaries from the bytecode alone.

`kind` selects a construct from the table below. `flags` is reserved and must be zero. `type_id` and `member` describe the [binding](#bindings). Only these two fields use `0xffff` to mean absence.

### Kinds

| `kind` | Construct               |
| ------ | ----------------------- |
| 0      | `def`                   |
| 1      | `ref`                   |
| 2      | `pattern`               |
| 3      | `capture`               |
| 4      | `grammar_field`         |
| 5      | `negated_grammar_field` |
| 6      | `predicate`             |
| 7      | `quantifier`            |
| 8      | `sequence`              |
| 9      | `unlabeled_alternation` |
| 10     | `labeled_alternation`   |
| 11     | `alternative`           |
| 12     | `capture_type`          |

All other `kind` values are invalid. Kinds 5 and 6 are accepted but have no compiler-emitted entries.

Definitions, references, node patterns, sequences, alternations, and alternatives cover their complete syntax. An alternative's range includes its label when present. Node patterns include named nodes, anonymous nodes, and wildcards.

The smaller constructs have narrower ranges:

| Construct     | Query text covered             |
| ------------- | ------------------------------ |
| Capture       | The `@name` token              |
| Grammar field | The field name, without `:`    |
| Quantifier    | The quantifier operator        |
| Capture type  | The complete `:: T` annotation |

The capture-type rule also covers `:: text` and `:: bool`. A discard capture `@_` has no capture entry, although its inner constructs can still have entries.

An entry need not have a corresponding span effect. Capture-type entries in compiler output describe the annotation without opening an execution occurrence of their own.

### Bindings

The two binding fields distinguish no binding, a type binding, and a member binding:

| `type_id` | `member`     | Binding |
| --------- | ------------ | ------- |
| `0xffff`  | `0xffff`     | None    |
| Type ID   | `0xffff`     | Type    |
| Type ID   | Member index | Member  |
| `0xffff`  | Member index | Invalid |

A present `type_id` selects an existing `TypeDefs` entry. A present `member` selects an existing `TypeMembers` entry by its absolute table index and requires a present `type_id`. In emitted output, that type is the member's parent record or variant:

```text
Span entry              TypeDefs
  type_id ------------> parent Record or Variant
                              |
                              | contains
                              v
  member -------------> TypeMembers entry ----> field or payload type
```

The reader checks the two indices and the requirement for a type alongside a member. It does not check that the type is a record or variant, that it owns the member, or that the binding suits the span's `kind`.

Value-producing definitions and references can bind to their result type. Captures that assign a field bind to that field's member. Alternatives that construct a variant case bind to that case's member. Capture-type annotations bind to the indicated type. Suppressed result syntax can retain inspection entries without bindings.

A binding describes result structure. It does not establish that the construct executes or produces a value on every match.

### Inspection occurrences

The committed span effects of a successful match determine its inspection occurrences. Each opener creates one occurrence for its span ID. Repetition or recursion can therefore produce several occurrences of the same entry. Nested open and close effects establish occurrence nesting.

An occurrence's optional document range includes its first byte and excludes its end byte. Contributions follow effect order:

| Effect                         | Range contribution                        |
| ------------------------------ | ----------------------------------------- |
| `SpanStartAt`                  | Current node, to the new occurrence       |
| `SpanStart`                    | None                                      |
| `Node`, `NodeText`, `NodeBool` | Current node, to the innermost occurrence |
| `ScalarMark`                   | Marked node, to the innermost occurrence  |
| `SpanEnd`                      | Closed range, to its enclosing occurrence |

Node output and marks contribute only when an occurrence is open. `ScalarMark` also needs an open `Scalar` frame. Closing a scalar or attaching a completed value does not add its accumulated range to the current occurrence.

A span that opens and closes between `ScalarOpen` and `TextClose` therefore includes marks made while it was open, even though the scalar value is finished later.

The occurrence's range runs from the earliest contributing start to the latest contributing end. It is absent if nothing contributes. A real zero-byte node contributes a present empty range.

Scalar provenance remains available even when lower-detail inspection entries were omitted. A text result's provenance describes the text it selects. A boolean produced from matched nodes can have provenance, while an explicit boolean with no matched node has none. An absent result has no invented document range. The [scalar and span effects](04-effects.md) define these contributions, suppression, and bracket pairing.

Member bindings connect occurrences to declared fields or cases. The executed effects determine runtime result paths, list indices, and document ranges. None of those are encoded by the static binding alone.

### Capacity

With inspection enabled, the compiler considers entries for reachable definitions in five detail tiers:

| Tier | Kinds                                                           |
| ---- | --------------------------------------------------------------- |
| 0    | `def`                                                           |
| 1    | `capture`                                                       |
| 2    | `pattern`, `ref`                                                |
| 3    | `alternative`, `quantifier`, `sequence`, both alternation kinds |
| 4    | `grammar_field`, `capture_type`                                 |

Lower-numbered tiers have priority. A tier is kept only if all its entries fit within the 1024-entry limit after the earlier retained tiers. Otherwise the whole tier is omitted and consumes no capacity, so a later, smaller tier can still fit. Retained entries keep their relative order. They need not follow source-position or containment order.

For example, with 1000 entries retained, a tier of 30 entries is omitted in full. A later tier of 20 entries still fits, bringing the total to 1020.

If any nonempty tier is omitted, the compiler emits the `inspection_spans_degraded` diagnostic. Omitted entries and references to them are absent from the bytecode. Kinds 5 and 6 have no compiler candidates and do not participate in this rule.

### Validation

The reader rejects more than 1024 entries, an unknown `kind`, nonzero `flags`, or a reversed query range. Present bindings obey the index and absence rules above. Every span effect refers to an existing entry and follows the [effect balance rules](04-effects.md#valid-effect-sequences).

Entries need not be unique, ordered, nested, or referenced by instructions. The reader's checks do not establish the external source or binding relationships described above. Writers supply those relationships.
