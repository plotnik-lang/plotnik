# Instructions and entry points

- [Design](#design)
- [Implementation](#implementation)
  - [Header and opcodes](#header-and-opcodes)
  - [Addresses and instruction boundaries](#addresses-and-instruction-boundaries)
  - [Match encoding](#match-encoding)
    - [Match semantics](#match-semantics)
    - [Accepted and emitted forms](#accepted-and-emitted-forms)
  - [Navigation](#navigation)
  - [Predicates](#predicates)
  - [Calls and returns](#calls-and-returns)
    - [Call1](#call1)
    - [CallN](#calln)
    - [Return](#return)
    - [Execution contract](#execution-contract)
  - [Entry points](#entry-points)
  - [Valid control flow](#valid-control-flow)
    - [Cursor depth](#cursor-depth)
    - [Empty paths](#empty-paths)
    - [Acceptance and validation scope](#acceptance-and-validation-scope)

## Design

The program follows a cursor through the syntax tree. The cursor identifies the node currently being examined. Execution begins at the tree root, using the starting instruction selected by the entry point. A query reaches descendants only through explicit navigation.

A `Match` instruction selects a candidate node and tests its constraints. If the node passes, its [effects](04-effects.md) perform actions such as capturing the node or adding a field to the result. Its successor addresses then select the instructions that can continue the match. These explicit links form the program's control-flow graph, so physical adjacency in the bytecode does not imply execution order.

The `Match` forms differ in how much space they reserve for operands. Their names include their byte length: `Match16` occupies 16 bytes, for example.

Those explicit links also let the compiler arrange the stored bytecode around CPU cache lines. Linear paths are placed together, and connected instructions can fill unused space in earlier 64-byte blocks. Each instruction fits wholly inside one block, so reading its header and immediate operands needs at most one line. Small instructions can share that line.

For example, a 48-byte instruction can share a block with a 16-byte instruction. A following 24-byte instruction needs another block:

```text
byte offset 0                                   48          64
            +-----------------------------------+-----------+
            | Match48                           | Match16   |
            +-----------------------------------+-----------+
            |<---------------- 64 bytes ------------------->|

            64                88                            128
            +-----------------+-----------------------------+
            | Match24         | Remaining space             |
            +-----------------+-----------------------------+
            |<---------------- 64 bytes ------------------->|
```

The remaining space can hold other instructions or zero-filled words. This packing keeps related instructions close in the stored bytes without changing which successor has priority. The VM builds separate execution tables at load time and runs from those tables. The 64-byte packing describes the stored instructions, not the decoded tables.

A navigation command determines both where a search starts and which nodes it can pass over. Consider this fragment of a tree, with the cursor on the binary expression:

```text
binary_expression  <-- current cursor
|
+-- identifier "a"
|
+-- "+"
|
+-- identifier "b"
```

A `Match` with `Down` navigation and an identifier constraint first selects `a`. Suppose the next instruction is a `StayExact` match requiring that node's text to equal `"b"`. It tests only the selected node, so it fails for `a`. The earlier `Down` left another choice: a later sibling. It can pass over the `+` node and select `b`, where the text test succeeds. Any effects from the failed attempt at `a` are discarded. With `DownExact`, the same fragment fails because that navigation permits no later candidate.

A `Match` can also offer several successor instructions at one candidate. Their encoded order is their preference order. All successors at the current candidate take preference over retrying a later sibling. Only a complete successful path accepts, so an early local match does not commit its result. `Epsilon` connects paths without moving or testing the cursor, while still allowing effects.

Calls share a reusable part of the graph, called a body. A body returns through numbered exits called ports. For each port, the call site supplies a continuation: the address at which its own execution resumes. The same body can therefore continue differently for different callers:

```text
call site A --target--> body T --Return 1--+
     |                                     |
     +-- continuation 0: A0                |
     |                                     |
     +-- continuation 1: A1 <--------------+
```

Here `Return` selects port 1 at call site A and continues at A1. A different call site can give port 1 a different destination. Returning does not restore the cursor. The body's navigation determines where the cursor is when its continuation begins.

The call also declares who selects its entry node. With caller ownership, the call performs navigation and its grammar-field check before entering the body. With callee ownership, the body performs that selection and can return without matching a node. Each port declares whether its path matched a node, called consuming, or returned without one, called empty. Matching the current node counts as consuming even when the cursor does not move. The port number itself does not imply either outcome.

If the body can match nothing, navigating first could fail before it reaches that empty path, for example when `Down` finds no child. Callee ownership lets the body decide whether navigation is needed.

An entry point also chooses how to finish the body's result, its boundary mode. It can retain the body's result, capture the final cursor node, or collect the body's fields into an outer record. Ordinary calls to the same body do not apply this boundary again. This lets one body serve both entry execution and nested calls without adding an extra outer value at every call.

## Implementation

Execution begins at the selected entry target with the cursor at the tree root. Offsets below are relative to the start of the enclosing instruction or entry record.

### Header and opcodes

Every instruction starts with this header byte. The segment is zero. The opcode determines the meaning of the flags.

```text
bits     7..6       5..4       3..0
       +----------+----------+----------+
       | segment  | flags    | opcode   |
       +----------+----------+----------+
```

| Opcode | Instruction | Bytes | Payload slots |
| ------ | ----------- | ----- | ------------- |
| `0x0`  | `Match8`    | 8     | None          |
| `0x1`  | `Match16`   | 16    | 4             |
| `0x2`  | `Match24`   | 24    | 8             |
| `0x3`  | `Match32`   | 32    | 12            |
| `0x4`  | `Match48`   | 48    | 20            |
| `0x5`  | `Match64`   | 64    | 28            |
| `0x6`  | `Call1`     | 8     | None          |
| `0x7`  | `CallN`     | 24    | None          |
| `0x8`  | `Return`    | 8     | None          |

Payload capacities count `u16` slots in extended `Match` forms.

Opcodes `0x9`–`0xf` are invalid.

### Addresses and instruction boundaries

Instruction addresses are `u16` indices counting 8-byte words from the start of the `Instructions` section. The section contains exactly `instruction_word_count` words, filled by complete instructions. An instruction that crosses the section end is invalid.

Every control-flow target addresses an instruction's first word. Interior words of a multiword instruction are not targets. There is no implicit fallthrough between adjacent instructions.

For example, a `Match24` at word 0 occupies words 0, 1, and 2. A following `Return` begins at word 3. Words 1 and 2 cannot be targets:

```text
word address 0           1           2           3
             +-----------------------------------+-----------+
             | Match24                           | Return    |
             +-----------------------------------+-----------+
byte offset  0           8           16          24          32
```

A `Match` with no successor is terminal. It can accept an entry-only branch alongside the body's required `Return` paths. A called body must exit through `Return`. Zero can mark a terminal match or name an instruction, depending on the operand:

| Operand                     | Zero                  | Nonzero             |
| --------------------------- | --------------------- | ------------------- |
| Entry target, call target   | Instruction address 0 | Instruction address |
| `Match8.next`               | Terminal match        | Successor address   |
| Extended `Match` successor  | Invalid               | Successor address   |
| Used call continuation      | Invalid               | Successor address   |
| Unused `CallN` continuation | Required              | Invalid             |

Writers may leave whole zero-filled words between instructions. Each such word is a terminal `Match8` with `Epsilon` navigation. It retains an address and is subject to the same control-flow rules if referenced. A target also used as a `Match` successor or call continuation needs a nonzero address.

The compiler keeps every instruction within a 64-byte block measured from the start of `Instructions`, whose section start is also 64-byte aligned. Unused whole words before later instructions are zero. `instruction_word_count` includes those gaps and ends after the final instruction. The reader requires complete instructions but does not reject one merely because it crosses a 64-byte boundary.

### Match encoding

```text
byte       0        1        2..3       4..5       6..7
         +--------+--------+----------+----------+----------+
Match8   | header | nav    | kind     | field    | next     |
         +--------+--------+----------+----------+----------+
Extended | header | nav    | kind     | field    | counts   |
         +--------+--------+----------+----------+----------+

byte 8 through end: extended payload
```

`nav` selects the navigation command. `kind` tests the node's grammar kind, and `field` tests its role in its parent. Header bits 5–4 distinguish named nodes, such as `identifier`, from anonymous tokens, such as `+`. This distinction is the node class. Grammar field zero means no constraint. `Match8` has no extended payload.

The node class and node kind jointly define the kind constraint:

| Class | Kind zero          | Kind nonzero                     |
| ----- | ------------------ | -------------------------------- |
| `00`  | Any node           | Invalid                          |
| `01`  | Any named node     | Named node with this kind ID     |
| `10`  | Any anonymous node | Anonymous node with this kind ID |
| `11`  | Invalid            | Invalid                          |

Kind `0xfffe`, Tree-sitter's internal `_ERROR`, is invalid. The public `ERROR` kind is `0xffff` and is representable. A specific kind does not replace the named/anonymous condition.

The extended `Match` counts word is:

```text
bits    15..12    11..9      8..4         3         2..0
       +---------+---------+------------+---------+-----------+
       | effects | negated | successors | missing | predicate |
       +---------+---------+------------+---------+-----------+
```

The three counts allow up to 15 effects, 7 negated fields, and 31 successors. A negated field requires the candidate to have no child in that grammar field. The predicate field selects a test of the node's source text, with zero meaning no predicate. The missing bit independently requires a node that Tree-sitter inserted to recover from absent syntax.

The payload starts at byte 8. Its parts are contiguous `u16` slots in this order:

```text
+--------------+----------------+-----------+------------+--------+
| effect words | negated fields | predicate | successors | unused |
+--------------+----------------+-----------+------------+--------+
```

Each effect, negated field, and successor occupies one slot. A present predicate occupies one. The counts share the selected opcode's capacity, which the used slots cannot exceed. Negated field IDs are nonzero. Repeated successors and negated fields are accepted.

For example, two effects, one negated field, a predicate, and two successors use six slots. They fit in `Match24` with two unused slots, but cannot fit in `Match16`.

#### Match semantics

For navigation other than `Epsilon`, a candidate satisfies the `Match` when all encoded conditions hold:

- The node class and kind match.
- A nonzero field operand equals the candidate's field in its parent.
- None of the negated fields has a child on the candidate.
- A present predicate succeeds.
- A set missing bit requires a Tree-sitter missing node. A clear bit places no condition on missing status.

`Epsilon` neither moves nor tests the cursor. Its kind, field, missing, negated-field, and predicate conditions have no execution effect. Their encodings and operand references remain subject to structural validation.

When a candidate passes, effects take place in encoded order. A `Match` with no successors accepts entry execution. Otherwise, successors are alternatives in encoded order. All alternatives begin with the state after that `Match`'s effects. A failed alternative restores that state before another alternative is considered. Retrying a later node candidate also rolls back the effects and call state of the abandoned candidate. Only the first complete successful path commits a result.

#### Accepted and emitted forms

A `Match` form can be larger than its operands require. The reader ignores unused bytes in extended `Match` forms, including nonzero bytes.

The compiler emits `Match8` when there are no effects, negated fields, predicate, or missing constraint and at most one successor. Otherwise it emits the smallest extended form with sufficient capacity. It zeroes unused payload bytes and omits ignored constraints from `Epsilon` matches. A missing-only `Match` occupies at least 16 bytes.

The binary reader does not require kind or field operands to appear in the [symbol tables](06-tables.md), or check them against a loaded grammar. Apart from the kind restrictions above and nonzero negated fields, these operands remain grammar IDs used directly during matching. Execution requires IDs from the grammar that produced the supplied syntax tree.

### Navigation

For bytes below `0x80`, only the following values are valid. A candidate's skip policy determines whether that candidate may be passed over to reach its next sibling after rejection.

| Byte | Name                  | Initial candidate | Skip policy |
| ---- | --------------------- | ----------------- | ----------- |
| 0    | `Epsilon`             | None              | None        |
| 1    | `Stay`                | Current node      | Any         |
| 2    | `StayExact`           | Current node      | Exact       |
| 3    | `Next`                | Next sibling      | Any         |
| 4    | `NextSkip`            | Next sibling      | Trivia      |
| 5    | `NextSkipExtras`      | Next sibling      | Extras      |
| 6    | `NextExact`           | Next sibling      | Exact       |
| 7    | `Down`                | First child       | Any         |
| 8    | `DownSkip`            | First child       | Trivia      |
| 9    | `DownSkipExtras`      | First child       | Extras      |
| 10   | `DownExact`           | First child       | Exact       |
| 11   | `ChildlessSkipTrivia` | Current node      | Exact       |
| 12   | `ChildlessSkipExtras` | Current node      | Exact       |
| 13   | `ChildlessExact`      | Current node      | Exact       |

`Epsilon` performs no movement or node check. The Childless commands leave the cursor unchanged and assert a condition on all its children: `ChildlessSkipTrivia` requires every child to be trivia, `ChildlessSkipExtras` requires every child to be extra, and `ChildlessExact` requires no children.

| Policy | Nodes that may be skipped             |
| ------ | ------------------------------------- |
| Any    | Every node                            |
| Trivia | Anonymous nodes or Tree-sitter extras |
| Extras | Tree-sitter extras only               |
| Exact  | None                                  |

Tree-sitter marks nodes such as comments as extras when its grammar permits them between ordinary syntax. Plotnik's Trivia policy also permits skipping anonymous tokens. The policy applies to the node being skipped, not to the next candidate. An exact match has only its initial candidate. An absent initial child/sibling, a failed assertion, or exhaustion of legal candidates fails the instruction.

An `Up` byte has its high bit set:

```text
bits     7       6..5      4..0
       +-------+---------+-------------+
       | 1     | mode    | level count |
       +-------+---------+-------------+
```

The level count is 1–31. Encodings with level zero are invalid.

| Mode | Name           | Allowed later siblings |
| ---- | -------------- | ---------------------- |
| 0    | `Up`           | Any                    |
| 1    | `UpSkipTrivia` | Trivia only            |
| 2    | `UpSkipExtras` | Extras only            |
| 3    | `UpExact`      | None                   |

Every level left needs a parent, and every departing node's later siblings have to satisfy the selected mode.

Successful `Up` navigation selects the ancestor at the encoded level count above the initial node. A failed ascent leaves no accepted movement. `Match` constraints on the resulting ancestor use the Any skip policy, including for `UpExact`. The `Up` mode constrains the levels left, not the subsequent ancestor-kind check.

Non-exact `Down` and `Next` matches retain their later sibling candidates when the accepted candidate itself is skippable under the policy. Failure farther along the path can therefore select a later candidate. Their successors at the current candidate take preference over later candidates. `Stay` and `Up` matches may skip immediately rejected candidates, but do not retain later candidates after acceptance. Childless and exact matches have no sibling retry.

### Predicates

The three-bit predicate field in the counts word selects the operation and the operand's table. Zero means no predicate and no operand. A nonzero choice adds one `u16` operand to the payload, after effects and negated fields and before successors.

Predicates test the candidate node's exact source slice. Text uses UTF-8 bytes without case folding, normalization, or trimming.

| Predicate | Condition      | Operand      |
| --------- | -------------- | ------------ |
| 0         | No predicate   | Absent       |
| 1         | Equal          | String index |
| 2         | Unequal        | String index |
| 3         | Prefix         | String index |
| 4         | Suffix         | String index |
| 5         | Substring      | String index |
| 6         | Regex match    | Regex index  |
| 7         | No regex match | Regex index  |

For `(identifier == "foo")`, if `"foo"` has string index 12, the predicate field is 1 and its payload operand is 12. The operator determines which table contains the operand, so no separate regex flag is stored.

Equality compares the entire source slice to the referenced string. Prefix and suffix require that string at the start or end of the slice. Substring requires a contiguous occurrence within the slice. Regex operators test whether the compiled DFA for the referenced regex finds a match in the slice.

String operands are less than `str_table_count`, including the real reserved string at index zero. Regex operands are nonzero and less than `regex_table_count`. Matching searches the complete node slice as an unanchored haystack, subject to any anchors encoded by the [DFA](06-tables.md).

### Calls and returns

A call has one target and one to eight continuations. A return port is an integer local to that target, not a global address or a fixed category such as consuming versus empty. Ports are numbered consecutively from zero, one per continuation. Each port's consumed bit is at the same position in the consumed mask.

Ownership selects where the call's entry navigation and field constraint take effect:

- Caller ownership, flag 0: the call selects a node using its navigation and field before entering the target. The body returns at its entry depth, and every port is consuming.
- Callee ownership, flag 1: the call enters the target without cursor movement or a field check. The target implements the exact navigation and optional field declared by the call.

Calls and `Return` contract metadata accept only navigation bytes 1–10. `Epsilon`, Childless, and `Up` are invalid here. Field zero means no constraint.

#### Call1

```text
byte    0        1        2..3      4..5           6..7
       +--------+--------+---------+--------------+--------+
       | header | nav    | field   | continuation | target |
       +--------+--------+---------+--------------+--------+
```

Header bit 4 stores ownership and bit 5 stores whether port 0 consumed a node. The port-0 continuation is nonzero. Caller ownership requires bit 5 set. Callee ownership allows either value.

#### CallN

```text
byte    0        1        2..3      4..5     6       7
       +--------+--------+---------+--------+-------+---------------+
       | header | nav    | field   | target | arity | consumed mask |
       +--------+--------+---------+--------+-------+---------------+
byte    8..9         10..11        12..21                    22..23
       +------------+------------+-------------------------+------------+
       | return[0]  | return[1]  | return[2] ... return[6] | return[7]  |
       +------------+------------+-------------------------+------------+
```

Header bit 4 stores ownership and bit 5 is zero. `arity` is the number of used return ports, from 2 through 8. The eight return slots are `u16` continuations indexed by local port. Used slots are nonzero, and unused slots are zero. Mask bits at or above `arity` are zero. Caller ownership requires every used consumed bit set. Continuations may share an address.

#### Return

```text
byte    0        1        2        3        4..5      6..7
       +--------+--------+--------+--------+---------+---------+
       | header | port   | nav    | zero   | field   | zero    |
       +--------+--------+--------+--------+---------+---------+
```

Header bit 4 stores ownership and bit 5 is zero. The local port is 0–7, and both reserved fields are zero. A caller-owned `Return` encodes `Stay` and field zero. A callee-owned `Return` encodes the navigation and field of its body's entry contract.

#### Execution contract

A caller-owned `Stay` call checks its field only at the current node. A caller-owned `Down`/`Next` call selects the first candidate satisfying its field, under the navigation's skip policy. Non-exact `Down`/`Next` calls permit later legal candidates if the callee or subsequent continuation fails. `Stay` and exact calls have one candidate. Callee-owned calls delegate all entry selection and retry behavior to their target.

Each active call retains its call site. A `Return` through port `i` selects that site's continuation `i`. It does not restore the cursor or apply entry boundary effects. `Return` contract metadata and consumed bits are validation data, not additional runtime movement. With no active call, only port zero accepts the entry point.

### Entry points

The `EntryPoints` section contains `entry_points_count` records of 8 bytes:

```text
byte    0..1       2..3       4..5          6..7
       +----------+----------+-------------+----------+
       | name     | target   | result type | boundary |
       +----------+----------+-------------+----------+
```

The name is a nonzero string index below `str_table_count`. The target is an instruction address, and the result type ID is less than `type_defs_count`.

| Boundary | Name          | Before body  | After acceptance |
| -------- | ------------- | ------------ | ---------------- |
| 0        | `Passthrough` | No effect    | No effect        |
| 1        | `Node`        | No effect    | `Node`           |
| 2        | `Record`      | `RecordOpen` | `RecordClose`    |

`Passthrough` retains the body's value, or absent if none. The `Node` boundary captures the final cursor node. The `Record` boundary opens an empty record before the body runs and closes it after acceptance, collecting the fields the body produces.

Other boundary values are invalid. Boundary effects obey the same [effect rules](04-effects.md) as encoded effects. They belong only to entry execution. Ordinary and recursive calls to the same target do not apply them.

The compiler emits selectable definitions in definition order. A selectable definition's root matches exactly one node. Sequence- and quantifier-rooted fragments are not entries. The emitted boundary is `Record` for a direct `Record` result, `Node` for a direct `Node` result, and `Passthrough` for other results or match-only definitions. The completed value has to match the declared [type](03-types.md).

Readers validate the name and type references, target, boundary value, and effect discipline. They do not infer a boundary mode from the declared type or establish result-type soundness. The compiler supplies unique entry names in definition order. The reader does not check either property.

### Valid control flow

A body begins at an entry target or call target, called its root. Its local paths follow `Match` successors and call continuations. A nested call's target starts a separate body. Calls to one root agree on ownership, arity, and consumed mask. Callee-owned calls also agree on navigation and field. Entry targets have the caller-owned contract, arity one, and consuming port zero.

The `Return` instructions reachable within a body agree on the body's contract. Together they cover every declared port, with none missing and none outside the declared range. For a caller-owned root, the uniform contract is caller-owned `Stay` with field zero, regardless of the individual calls' own navigation and field. For a callee-owned root, `Return` metadata equals the navigation and field declared by the calls. Reachability for these checks follows all encoded alternatives, without using node or predicate conditions to discard paths.

#### Cursor depth

Depth is relative to a body root, whose depth is zero. Local paths reaching the same instruction have the same depth. A `Match` changes depth according to its navigation. `Down` adds one level, `Up` subtracts its level count, and every other command leaves depth unchanged. These names include every mode in each family.

Two paths can join after one descends and climbs back while the other stays at the original depth:

```text
             +-- Down --> depth 1 -- Up 1 -+
             |                             |
root (0) ----+                             +--> join (0)
             |                             |
             +-- Stay ---------------------+
```

Without the `Up`, the upper path reaches the join at depth 1 instead of 0, and the reader rejects it.

A call's consuming ports contribute the depth change of its declared navigation. Empty ports leave depth unchanged. Required `Return` depths are:

| Ownership | Port      | Entry navigation | `Return` depth |
| --------- | --------- | ---------------- | -------------- |
| Caller    | Any       | Any              | 0              |
| Callee    | Empty     | Any              | 0              |
| Callee    | Consuming | `Down`           | 1              |
| Callee    | Consuming | `Stay` or `Next` | 0              |

A terminal `Match`'s depth after navigation equals the required exit depth for port zero. Relative intermediate depths are not independently required to be nonnegative. Actual tree navigation still fails where a requested ancestor or child does not exist.

#### Empty paths

The reader tracks whether each local path has performed a match. Every body root begins empty, even though the cursor already points to a tree node. An `Epsilon` match or empty call port leaves this status unchanged. Any other `Match` or consuming call port makes it nonempty, including `Stay`, Childless, and `Up` matches.

[Effects that read the cursor](04-effects.md), such as capturing its node, are invalid while the path remains empty. This also holds under suppression, which temporarily disables result construction while matching continues. A `Node` entry boundary cannot accept through an empty path. A callee is checked from its own initially empty path even when its caller has already selected a node.

#### Acceptance and validation scope

Every body needs its complete reachable `Return` contract. A terminal `Match` alone cannot replace that contract. Called bodies reachable from entries return through `Return`. A terminal `Match` in such a body is invalid. A root-only body may have a terminal branch alongside its required `Return` branches, provided depth and effect validity hold.

Readers check structural encodings and references throughout the stream. `Return` contracts, cursor depths, and empty paths are checked from every entry target and every encoded call target. Result-effect paths are checked from entry targets and the calls reachable from them, as specified in [Effects](04-effects.md). Instructions outside those reachable bodies still need valid encodings, but their effect sequences are not checked.

Consumed bits describe actual node consumption, and each callee-owned target implements its declared entry navigation and field constraint. The reader's contract and depth checks do not prove those claims. Equal depths cannot distinguish consumed and empty horizontal paths. Matching call and `Return` metadata cannot establish that the body tests the declared field.
