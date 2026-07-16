# Spans Section

The Spans section stores query-side inspection metadata. Runtime span effects
refer to entries by index; the playground joins those ids to query spans,
result paths, type/member bindings, and execution-trace records.

## Entry Layout

Each entry is 16 bytes, little-endian:

| Offset | Size | Field       | Meaning                                 |
| ------ | ---- | ----------- | --------------------------------------- |
| 0      | 2    | `source_id` | Query source index                      |
| 2      | 1    | `kind`      | `SpanKind` discriminant                 |
| 3      | 1    | `flags`     | Reserved, must be zero                  |
| 4      | 4    | `start`     | Query byte start                        |
| 8      | 4    | `end`       | Query byte end                          |
| 12     | 2    | `type_id`   | Type binding, or `0xFFFF` for none      |
| 14     | 2    | `member`    | TypeMembers index, or `0xFFFF` for none |

`start <= end`. Any binding value other than `0xFFFF` must be in range for its
table, and a live `member` requires a live `type_id` — a member with no type is
rejected at load.

## Kinds

| Value | Name                    |
| ----- | ----------------------- |
| 0     | `def`                   |
| 1     | `ref`                   |
| 2     | `pattern`               |
| 3     | `capture`               |
| 4     | `grammar_field`         |
| 5     | `negated_grammar_field` |
| 6     | `predicate`             |
| 7     | `quantifier`            |
| 8     | `sequence`              |
| 9     | `unlabeled_alternation` |
| 10    | `labeled_alternation`   |
| 11    | `alternative`           |
| 12    | `capture_type`          |

Values 9 and 10 encode `SpanKind::Alternation(Labeling::Unlabeled)` and
`SpanKind::Alternation(Labeling::Labeled)`; labeling does not create a second
syntax construct.

`capture_type` covers `:: T`, `:: text`, or `:: bool`. `negated_grammar_field` and
`predicate` are reserved for inspection detail; loaders accept the kind values,
but the compiler does not emit them yet.

## Span Effects

Instructions may carry three span effect kinds:

| Effect        | Meaning                                          |
| ------------- | ------------------------------------------------ |
| `SpanStartAt` | Open a span and snapshot the current cursor node |
| `SpanStart`   | Open a span without reading the cursor           |
| `SpanEnd`     | Close the innermost open span                    |

The effect payload is a `SpanId` and must be `< spans_count`. `SpanStartAt`
is position-sensitive like `Node`: lowering must place it only where the VM
cursor already points at the matched node. The compiler asserts this on the
fresh Thompson IR before optimization; later passes may move it only along
cursor-preserving epsilon chains.

The `text` and `bool` capture types contribute their exact runtime provenance independently
of inspection-detail spans. While a capture span is open, its scalar marks are
folded into a possibly absent document bounding range: a present string highlights
precisely the text it returns, a present boolean may carry its document range, and
`null`/fallback `false` has no invented range. A real zero-byte node retains a
real empty span. This remains exact even when lower-priority pattern spans
are degraded away.

Construction-time effect-stack validation tracks span depth, including inside
suppression scopes, so malformed bytecode with unbalanced span brackets is
rejected before execution. Span effects are still recorded under runtime
suppression: a bare `(Foo)` reference suppresses `Foo`'s result values but not
its result-provenance span events.

## Degradation

The compiler can emit at most 1024 spans because effect payloads are 10-bit.
When inspection is enabled and a query has more span-worthy constructs, spans
are admitted by detail tier. Higher-value tiers such as definitions, captures,
patterns, and references are kept ahead of lower-value detail such as fields
and capture types. Each tier is admitted or dropped as a whole; a large dropped
tier does not prevent a later smaller tier from being admitted if it still
fits. Dropped tiers are omitted from the Spans section and reported with the
`inspection_spans_degraded` warning; the module remains executable.
