# Spans Section

The Spans section stores query-side inspection metadata. Runtime span effects
refer to entries by index; the playground joins those ids to source hulls,
output paths, type/member bindings, and trace records.

## Entry Layout

Each entry is 16 bytes, little-endian:

| Offset | Size | Field     | Meaning                                 |
| ------ | ---- | --------- | --------------------------------------- |
| 0      | 2    | `source`  | Query source index                      |
| 2      | 1    | `kind`    | `SpanKind` discriminant                 |
| 3      | 1    | `flags`   | Reserved, must be zero                  |
| 4      | 4    | `start`   | Query byte start                        |
| 8      | 4    | `end`     | Query byte end                          |
| 12     | 2    | `type_id` | Type binding, or `0xFFFF` for none      |
| 14     | 2    | `member`  | TypeMembers index, or `0xFFFF` for none |

`start <= end`. `type_id` and `member` are validated independently: any value
other than `0xFFFF` must be in range for its table.

## Kinds

| Value | Name         |
| ----- | ------------ |
| 0     | `def`        |
| 1     | `ref`        |
| 2     | `pattern`    |
| 3     | `capture`    |
| 4     | `field`      |
| 5     | `neg_field`  |
| 6     | `predicate`  |
| 7     | `quantifier` |
| 8     | `sequence`   |
| 9     | `union`      |
| 10    | `enum`       |
| 11    | `branch`     |
| 12    | `annotation` |

`neg_field` and `predicate` are reserved for inspection detail; v10 loaders
accept the kind values, but the compiler does not emit them yet.

## Span Effects

Transitions may carry three span effect kinds:

| Effect        | Meaning                                          |
| ------------- | ------------------------------------------------ |
| `SpanStartAt` | Open a span and snapshot the current cursor node |
| `SpanStart`   | Open a span without reading the cursor           |
| `SpanEnd`     | Close the innermost open span                    |

The effect payload is a `SpanId` and must be `< spans_count`. `SpanStartAt`
is position-sensitive like `Node`: lowering must place it only where the VM
cursor already points at the matched node. Load-time effect-stack validation
tracks span depth, including inside suppression scopes, so malformed bytecode
with unbalanced span brackets is rejected before execution.
