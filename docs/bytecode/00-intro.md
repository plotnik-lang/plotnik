# Bytecode

- [Design](#design)
- [Implementation](#implementation)

## Design

The compiler turns a query into bytecode: instructions for walking the tree, testing nodes, and building captured values. For example, this query captures a JavaScript variable's name:

```plotnik
Q = (program
  (lexical_declaration
    (variable_declarator name: (identifier) @name)))
```

For `const answer = 42`, the relevant tree path is:

```text
program
  |
  +-- lexical_declaration
        |
        +-- variable_declarator
              |
              +-- name: identifier "answer" --> result.name
```

Instructions encode the navigation and node tests along that path. Capturing adds actions that save the matched node and put it in a result field. These actions are called effects. An instruction carries its effects alongside its tests, and they take place when those tests pass. Here they put the identifier into the `name` field.

Type tables describe the result's shape separately from its values. This query produces a record, `{ name: Node }`, where `Node` is a captured syntax-tree node. The bytecode stores the shape, while each match supplies the node. The same bytecode can match other documents parsed with the same grammar.

Instructions also contain links to the instructions that can follow them, their successors. These links form the matching program. An instruction keeps its tests, effects, and successor addresses together. Shared tables hold names and constants, so repeated strings and regexes do not inflate every instruction.

The stored layout is designed around 64-byte CPU cache lines. The compiler packs connected instructions together, and each instruction fits within one line with its tests, effects, and successors. Small instructions share a line. Shared data lives in separate sections with the same alignment. The VM decodes this packed form into separate execution tables at load time.

Inspection adds another use for effects: recording which part of the query matched which document text. Optional metadata identifies locations in the query, and effects mark when those locations execute. Here an inspector can associate the `@name` capture with `answer` in the document.

## Implementation

1. [Layout](01-layout.md): header, sections, padding, checksum, and references.
2. [Instructions](02-instructions.md): entry points, matching, and control flow.
3. [Types](03-types.md): result shapes, members, and names.
4. [Effects](04-effects.md): result construction and suppression.
5. [Spans](05-spans.md): query locations and what they matched in the document.
6. [Tables](06-tables.md): strings, compiled regexes, and grammar names.
