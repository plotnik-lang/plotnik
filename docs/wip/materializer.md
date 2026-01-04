# Materializer Architecture

## Overview

The materializer transforms VM effect logs into structured output values.

## Problem

The VM produces `RuntimeEffect<'t>` items containing `tree_sitter::Node<'t>` references. These are tied to the tree's lifetime, preventing:

- Aggregating values across multiple files
- Storing values beyond the tree's lifetime
- Clean API without lifetime pollution

## Design

### Effect Log

The VM produces a single stream of effects (defined in `runtime-engine.md`):

```rust
pub enum RuntimeEffect<'t> {
    Node(tree_sitter::Node<'t>),
    Text(tree_sitter::Node<'t>),
    Arr,
    Push,
    EndArr,
    Obj,
    Set(u16),       // member index
    EndObj,
    Enum(u16),      // variant index
    EndEnum,
    Clear,
    Null,
}

struct EffectLog<'t>(Vec<RuntimeEffect<'t>>);
```

Lifetime `'t` denotes the parsed tree-sitter tree (per project conventions).

The `Set` and `Enum` payloads are member indices into the type's members. Field/variant names come from type metadata (`TypesView` in `bytecode/module.rs`), not the effect itself.

### Materializer Trait

```rust
pub trait Materializer<'t> {
    type Output;

    fn materialize(
        &self,
        effects: &[RuntimeEffect<'t>],
        result_type: QTypeId,
    ) -> Self::Output;
}
```

- Context (source, type metadata) carried by the materializer struct
- `result_type` from `Entrypoint.result_type` provides nominal type info

### Value Types

#### NodeHandle (Lifetime-free)

Captures enough information to identify a node without holding a reference:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeHandle {
    pub start: u32,
    pub end: u32,
    pub kind_id: u16,
}

impl NodeHandle {
    pub fn from_node(node: tree_sitter::Node) -> Self {
        Self {
            start: node.start_byte() as u32,
            end: node.end_byte() as u32,
            kind_id: node.kind_id(),
        }
    }

    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start as usize..self.end as usize]
    }
}
```

#### Value (Output Type)

Self-contained with resolved strings. Implements `serde::Serialize`.

```rust
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Value {
    Null,
    String(String),
    Node(NodeHandle),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),  // preserves field order
    Tagged { tag: String, data: Box<Value> },
}
```

`Object` uses `Vec<(String, Value)>` to preserve field order from type metadata.

### Serialization Format

Per `lang-reference.md`, nodes serialize as:

```json
{
  "kind": "identifier",
  "text": "foo",
  "start": { "row": 0, "column": 0 },
  "end": { "row": 0, "column": 3 }
}
```

Tagged unions serialize as:

```json
{
  "$tag": "Assign",
  "$data": { "target": "x", "value": { ... } }
}
```

To serialize `NodeHandle` with positions, the serializer needs source text and the tree:

```rust
pub struct SerializeContext<'a> {
    pub source: &'a str,
    pub tree: &'a tree_sitter::Tree,
    pub node_types: &'a [&'a str],  // kind_id -> kind name
}
```

### ValueMaterializer

Produces `Value` with resolved strings. Uses existing `TypesView` and `StringsView` from `bytecode/module.rs`.

```rust
pub struct ValueMaterializer<'ctx> {
    source: &'ctx str,
    types: TypesView<'ctx>,
    strings: StringsView<'ctx>,
}

impl<'t> Materializer<'t> for ValueMaterializer<'_> {
    type Output = Value;

    fn materialize(
        &self,
        effects: &[RuntimeEffect<'t>],
        result_type: QTypeId,
    ) -> Value {
        // Walk effects, resolve member indices to field names
    }
}
```

### Materialization Algorithm

Walk the effect stream with a value stack:

```rust
fn materialize(&self, effects: &[RuntimeEffect<'t>], result_type: QTypeId) -> Value {
    let mut stack: Vec<ValueBuilder> = vec![];
    let mut current = ValueBuilder::new(result_type);

    for effect in effects {
        match effect {
            RuntimeEffect::Node(n) => {
                current.set_node(NodeHandle::from_node(*n));
            }
            RuntimeEffect::Text(n) => {
                let text = n.utf8_text(self.source.as_bytes()).unwrap();
                current.set_string(text.to_string());
            }
            RuntimeEffect::Null => {
                current.set_null();
            }
            RuntimeEffect::Obj => {
                stack.push(std::mem::replace(&mut current, ValueBuilder::object()));
            }
            RuntimeEffect::Set(idx) => {
                let field_name = self.resolve_member_name(*idx);
                current.set_field(field_name, stack.pop().unwrap().build());
            }
            RuntimeEffect::EndObj => {
                let obj = std::mem::replace(&mut current, stack.pop().unwrap());
                current.set_value(obj.build());
            }
            RuntimeEffect::Arr => {
                stack.push(std::mem::replace(&mut current, ValueBuilder::array()));
            }
            RuntimeEffect::Push => {
                current.push_item(stack.pop().unwrap().build());
            }
            RuntimeEffect::EndArr => {
                let arr = std::mem::replace(&mut current, stack.pop().unwrap());
                current.set_value(arr.build());
            }
            RuntimeEffect::Enum(idx) => {
                let tag = self.resolve_variant_name(*idx);
                stack.push(std::mem::replace(&mut current, ValueBuilder::tagged(tag)));
            }
            RuntimeEffect::EndEnum => {
                let tagged = std::mem::replace(&mut current, stack.pop().unwrap());
                current.set_value(tagged.build());
            }
            RuntimeEffect::Clear => {
                current.clear();
            }
        }
    }

    current.build()
}
```

## Usage

### Single-file Execution

```rust
let module = Module::from_path("query.ptkb")?;
let tree = parser.parse(source, None)?;

let effects = vm.execute(&module, &tree, entrypoint)?;

let materializer = ValueMaterializer::new(
    source,
    module.types(),
    module.strings(),
);
let value = materializer.materialize(&effects.0, entrypoint.result_type);

serde_json::to_writer(stdout, &value)?;
```

### Multi-file Aggregation

`Value` is lifetime-free (uses `NodeHandle`), so results can outlive individual trees:

```rust
let mut all_results: Vec<(PathBuf, Value)> = vec![];

for file in files {
    let tree = parser.parse(&file.source, None)?;
    let effects = vm.execute(&module, &tree, entrypoint)?;

    let materializer = ValueMaterializer::new(
        &file.source,
        module.types(),
        module.strings(),
    );
    let value = materializer.materialize(&effects.0, entrypoint.result_type);

    all_results.push((file.path.clone(), value));
    // tree dropped here, but value survives
}
```

## Files

| File                      | Purpose                                   |
| ------------------------- | ----------------------------------------- |
| `runtime/effect.rs`       | `RuntimeEffect` enum                      |
| `runtime/value.rs`        | `Value` enum, `NodeHandle`                |
| `runtime/materializer.rs` | `Materializer` trait, `ValueMaterializer` |
| `runtime/serialize.rs`    | `SerializeContext`, node serialization    |

## Related

- `runtime-engine.md` — VM architecture, effect log
- `lang-reference.md` — Output format specification
- `bytecode/effects.rs` — `EffectOp` bytecode encoding
- `bytecode/module.rs` — `TypesView`, `StringsView`
