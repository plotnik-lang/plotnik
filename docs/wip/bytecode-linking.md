# Bytecode Linking Design

## Scope

**Design** the bytecode format to support future post-compilation linking, but **implement only**:
- Emit linked bytecode (current behavior)
- Emit unlinked bytecode (new: StringIds in instructions)
- CLI dumps both for debugging
- Runtime executes only linked bytecode, rejects unlinked

**NOT implementing**: Actual relinking logic. The format supports it; we don't use it yet.

## Chosen Design: Header Flag + StringId References

**Key insight**: `StringId` and `NodeTypeId` are both `u16`. Instructions always store a `u16` in bytes 2-3 (node_type) and 4-5 (node_field). A header flag indicates how to interpret them.

### StringId(0) Reservation

Since `0` means "no constraint" (wildcard) in instruction bytes, `StringId(0)` can never be referenced by instructions. Reserve it as an easter egg:

```
strings[0] = "Beauty will save the world"   // Dostoevsky, The Idiot
strings[1] = first actual string
strings[2] = second actual string
...
```

Actual string references use 1-based indices. The easter egg sits at index 0, visible to anyone who hexdumps the bytecode.

### Unlinked Bytecode
```
Header: linked = false
Match instruction bytes 2-3: StringId (index into string table)
Match instruction bytes 4-5: StringId (index into string table)
node_types section: empty (reserved)
node_fields section: empty (reserved)
```

### Linked Bytecode (emitted via `LinkedQuery::emit()`)
```
Header: linked = true
Match instruction bytes 2-3: NodeTypeId (grammar ID)
Match instruction bytes 4-5: NodeFieldId (grammar ID)
node_types section: [(NodeTypeId, StringId), ...] for verification
node_fields section: [(NodeFieldId, StringId), ...] for verification
```

### Runtime Behavior
- If `linked = false`: reject execution, require linking first
- If `linked = true`: execute directly, optionally verify symbol tables match loaded grammar

---

## Current Architecture

### Compilation Flow
```
Query → Parse → Analyze → [Link against grammar] → Compile → Emit bytecode
                              ↓
                         node_type_ids: HashMap<Symbol, NodeTypeId>
                         node_field_ids: HashMap<Symbol, NodeFieldId>
```

### Key Structures

**MatchIR** (`bytecode/ir.rs:74-93`):
```rust
pub struct MatchIR {
    pub node_type: Option<NonZeroU16>,  // Already numeric (tree-sitter ID)
    pub node_field: Option<NonZeroU16>, // Already numeric
    pub successors: Vec<Label>,         // Symbolic, resolved at emit
    // ...
}
```

**Match instruction wire format** (8 bytes minimum):
```
[0]   opcode + segment
[1]   nav command
[2-3] node_type (u16 LE, 0 = wildcard)  ← Target for relinking
[4-5] node_field (u16 LE, 0 = wildcard) ← Target for relinking
[6-7] next StepId or counts
```

### Two Emit Paths

| Path | `node_type_ids` | Result |
|------|-----------------|--------|
| `QueryAnalyzed::emit()` | `None` | All node_type/field = 0 (wildcard) |
| `LinkedQuery::emit()` | `Some(HashMap)` | Actual grammar IDs |

## Implementation Plan

### 1. Header Flag

Add `linked: bool` flag to `Header` struct:

```rust
// In bytecode/mod.rs or bytecode/header.rs
pub struct Header {
    // ... existing fields ...
    pub flags: u16,  // bit 0 = linked
}

impl Header {
    pub fn is_linked(&self) -> bool {
        self.flags & 0x01 != 0
    }
}
```

### 2. StringTableBuilder: Reserve Index 0

```rust
impl StringTableBuilder {
    pub fn new() -> Self {
        let mut builder = Self { strings: Vec::new(), ... };
        // Reserve index 0 for easter egg
        builder.strings.push("Beauty will save the world".to_string());
        builder
    }

    pub fn get_or_intern(&mut self, ...) -> StringId {
        // Indices now start at 1
        let id = StringId((self.strings.len()) as u16);  // No +1 needed, index 0 already occupied
        // ...
    }
}
```

### 3. Compiler: Store StringId When Unlinked

Modify `Compiler` to accept a "linked" flag and store `StringId` in unlinked mode:

```rust
fn resolve_node_type(&self, node: &ast::NamedNode) -> Option<NonZeroU16> {
    let type_name = node.node_type()?.text();

    if self.linked {
        // Current behavior: resolve to NodeTypeId
        self.node_type_ids?.get(type_name).map(|id| NonZeroU16::new(id.get()))
    } else {
        // New: store StringId instead
        let string_id = self.strings.get_or_intern(type_name);
        NonZeroU16::new(string_id.0)
    }
}
```

### 4. Emit Paths

| Method | `linked` flag | Instructions contain | Symbol sections |
|--------|---------------|---------------------|-----------------|
| `QueryAnalyzed::emit()` | `false` | StringId | empty |
| `LinkedQuery::emit()` | `true` | NodeTypeId | populated |

### 5. CLI: Dump Both Formats

`debug --bytecode` should display:
- Header linked flag
- For unlinked: show string names from StringId
- For linked: show grammar IDs with resolved names from symbol sections

### 6. Runtime: Reject Unlinked

```rust
fn load_bytecode(bytes: &[u8]) -> Result<Runtime, Error> {
    let header = Header::from_bytes(&bytes[..64]);
    if !header.is_linked() {
        return Err(Error::UnlinkedBytecode);
    }
    // ... continue loading
}
```

Future work (NOT now): Implement `link_bytecode()` that walks transitions and resolves StringIds.

## Files to Modify

| File | Changes |
|------|---------|
| `bytecode/mod.rs` | Add `flags` field to `Header`, `is_linked()` method |
| `query/emit.rs` | Reserve StringId(0) easter egg, set linked flag |
| `query/compile.rs` | Store StringId when unlinked (needs access to StringTableBuilder) |
| `plotnik-cli/src/commands/debug/mod.rs` | Display linked flag, decode instructions appropriately |
| `bytecode/dump.rs` | Update dump logic for linked/unlinked awareness |

## Documentation Updates

| File | Changes |
|------|---------|
| `docs/binary-format/01-overview.md` | Add `flags` field to header layout, document linked bit |
| `docs/binary-format/02-header.md` | Document `flags` field, `is_linked` semantics |
| `docs/binary-format/03-strings.md` | Document StringId(0) reservation (easter egg) |
| `docs/binary-format/06-transitions.md` | Clarify bytes 2-5 meaning differs based on linked flag |
| `docs/wip/bytecode.md` | Update WIP notes on linking design |
