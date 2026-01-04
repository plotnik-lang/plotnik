# Suppressive Captures Design

> **Status**: Ready for implementation  
> **Feature**: `@_` and `@_name` syntax for suppressing capture effects

## Overview

Suppressive captures allow matching patterns structurally without contributing to the output type or emitting effects. This is useful for:

1. **Structural matching without capture** - Match a pattern for structure but don't emit its captures
2. **Reusing definitions without their captures** - A definition like `Expr` has internal captures, but when using `(Expr)` you sometimes don't want those captures leaking up

## Syntax

```
@_        ; anonymous suppressive capture
@_name    ; named suppressive capture (name is documentation only)
```

Names like `@_foo` and `@_bar` never cause collisions - they're all equivalent to `@_`.

## Example

```
Expr = (binary_expression left: (_) @left right: (_) @right)

; Without suppression: @left, @right bubble up
Query = (statement (Expr) @expr)
; Output: { expr: Node, left: Node, right: Node }

; With suppression: only @expr, internal captures discarded  
Query = (statement (Expr) @_expr)
; Output: { expr: Node }  -- WRONG, @_expr doesn't create field either

; Correct usage - suppress internal captures, capture the node differently:
Query = (statement { (Expr) @_ } @expr)
; Output: { expr: Node }
```

## Semantics

1. Suppressive captures **don't contribute to output type**
2. All effects inside a suppressive capture are **suppressed** (not emitted to the effect log)
3. **Nesting is supported**: `@_outer` containing `@_inner` works correctly (depth counter)
4. The **inner pattern still matches structurally** - only effects are suppressed

---

## Implementation Plan

### Phase 1: Lexer & Parser

#### 1.1 Lexer (`parser/cst.rs`)

Add new token type after existing tokens (before `Id`):

```rust
/// Suppressive capture: @_ or @_name
#[regex(r"@_[a-zA-Z0-9_]*")]
SuppressiveCapture,
```

**Why before `Id`?** The `@` token is separate, but we want `@_` and `@_foo` to be recognized as a single token, not `@` + `Underscore` + `Id`.

#### 1.2 Parser (`parser/grammar/fields.rs`)

Modify `try_parse_capture` to handle the new token:

```rust
pub(crate) fn try_parse_capture(&mut self, checkpoint: Checkpoint) {
    if self.currently_is(SyntaxKind::At) {
        // Existing regular capture handling
        self.start_node_at(checkpoint, SyntaxKind::Capture);
        self.drain_trivia();
        self.parse_capture_suffix();
        self.finish_node();
    } else if self.currently_is(SyntaxKind::SuppressiveCapture) {
        // New: suppressive capture is a single token
        self.start_node_at(checkpoint, SyntaxKind::Capture);
        self.drain_trivia();
        self.bump(); // consume SuppressiveCapture token
        // Note: no type annotation allowed on suppressive captures
        self.finish_node();
    }
}
```

#### 1.3 AST (`parser/ast.rs`)

Add method to `CapturedExpr`:

```rust
impl CapturedExpr {
    /// Returns true if this is a suppressive capture (@_ or @_name).
    /// Suppressive captures match structurally but don't contribute to output.
    pub fn is_suppressive(&self) -> bool {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .any(|t| t.kind() == SyntaxKind::SuppressiveCapture)
    }
    
    // Existing name() method returns None for suppressive captures
    // since SuppressiveCapture token != Id token
}
```

### Phase 2: Type Inference

#### 2.1 Type Inference (`analyze/type_check/infer.rs`)

Modify `infer_captured_expr` to skip suppressive captures:

```rust
fn infer_captured_expr(&mut self, cap: &CapturedExpr) -> TermInfo {
    // Suppressive captures don't contribute to output type
    if cap.is_suppressive() {
        // Still infer inner for structural validation, but don't create fields
        return cap.inner()
            .map(|i| self.infer_expr(&i))
            .map(|info| TermInfo::new(info.arity, TypeFlow::Void))
            .unwrap_or_else(TermInfo::void);
    }
    
    // ... existing logic for regular captures
}
```

**Key point**: We return `TypeFlow::Void` so no fields bubble up, but we still validate the inner expression.

### Phase 3: Bytecode

#### 3.1 Effect Opcodes (`bytecode/effects.rs`)

Add new opcodes:

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum EffectOpcode {
    Node = 0,
    Arr = 1,
    Push = 2,
    EndArr = 3,
    Obj = 4,
    EndObj = 5,
    Set = 6,
    Enum = 7,
    EndEnum = 8,
    Text = 9,
    Clear = 10,
    Null = 11,
    SuppressBegin = 12,  // NEW
    SuppressEnd = 13,    // NEW
}
```

Update `from_u8`:

```rust
fn from_u8(v: u8) -> Self {
    match v {
        // ... existing
        12 => Self::SuppressBegin,
        13 => Self::SuppressEnd,
        _ => panic!("invalid effect opcode: {v}"),
    }
}
```

#### 3.2 Update Binary Format Docs (`docs/binary-format/06-transitions.md`)

Add to the EffectOp table:

```markdown
| 12     | `SuppressBegin` | -                      |
| 13     | `SuppressEnd`   | -                      |
```

### Phase 4: Compilation

#### 4.1 Compile (`compile/expressions.rs`)

Modify `compile_captured_inner` to handle suppressive captures:

```rust
pub(super) fn compile_captured_inner(
    &mut self,
    cap: &ast::CapturedExpr,
    exit: Label,
    nav_override: Option<Nav>,
    outer_capture: CaptureEffects,
) -> Label {
    // Handle suppressive captures: emit SuppressBegin/End around inner
    if cap.is_suppressive() {
        let Some(inner) = cap.inner() else {
            // Bare @_ with no inner - just emit suppress markers (no-op)
            return self.emit_suppress_noop(exit, outer_capture);
        };
        
        // Emit: SuppressBegin → inner → SuppressEnd → outer_effects → exit
        
        // 1. Emit SuppressEnd + outer capture effects → exit
        let end_label = self.fresh_label();
        let mut end_effects = vec![EffectIR::simple(EffectOpcode::SuppressEnd, 0)];
        end_effects.extend(outer_capture.post);
        self.instructions.push(Instruction::Match(MatchIR {
            label: end_label,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: end_effects,
            successors: vec![exit],
        }));
        
        // 2. Compile inner expression → end_label
        // Inner gets NO capture effects (suppressed)
        let inner_entry = self.compile_expr_inner(
            &inner,
            end_label,
            nav_override,
            CaptureEffects::default(),
        );
        
        // 3. Emit SuppressBegin → inner_entry
        let begin_label = self.fresh_label();
        self.instructions.push(Instruction::Match(MatchIR {
            label: begin_label,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![EffectIR::simple(EffectOpcode::SuppressBegin, 0)],
            successors: vec![inner_entry],
        }));
        
        return begin_label;
    }
    
    // ... existing logic for regular captures
}

fn emit_suppress_noop(&mut self, exit: Label, outer_capture: CaptureEffects) -> Label {
    // Bare @_ without inner - just pass through outer effects
    if outer_capture.post.is_empty() {
        return exit;
    }
    let label = self.fresh_label();
    self.instructions.push(Instruction::Match(MatchIR {
        label,
        nav: Nav::Stay,
        node_type: None,
        node_field: None,
        pre_effects: vec![],
        neg_fields: vec![],
        post_effects: outer_capture.post,
        successors: vec![exit],
    }));
    label
}
```

### Phase 5: VM Execution

#### 5.1 Checkpoint (`engine/checkpoint.rs`)

Add suppress depth to checkpoint:

```rust
#[derive(Clone, Copy, Debug)]
pub struct Checkpoint {
    pub descendant_index: u32,
    pub effect_watermark: usize,
    pub frame_index: Option<u32>,
    pub recursion_depth: u32,
    pub ip: u16,
    pub skip_policy: Option<SkipPolicy>,
    pub suppress_depth: u16,  // NEW
}
```

#### 5.2 VM State (`engine/vm.rs`)

Add suppress depth to VM:

```rust
pub struct VM<'t> {
    cursor: CursorWrapper<'t>,
    ip: u16,
    frames: FrameArena,
    checkpoints: CheckpointStack,
    effects: EffectLog<'t>,
    matched_node: Option<Node<'t>>,
    exec_fuel: u32,
    recursion_depth: u32,
    limits: FuelLimits,
    skip_call_nav: bool,
    suppress_depth: u16,  // NEW
}
```

Initialize in `new()`:

```rust
pub fn new(tree: &'t Tree, trivia_types: Vec<u16>, limits: FuelLimits) -> Self {
    Self {
        // ... existing
        suppress_depth: 0,
    }
}
```

#### 5.3 Effect Emission (`engine/vm.rs`)

Modify `emit_effect` to handle suppression:

```rust
fn emit_effect<T: Tracer>(&mut self, op: EffectOp, tracer: &mut T) {
    use EffectOpcode::*;
    
    // Handle suppress control effects first
    match op.opcode {
        SuppressBegin => {
            self.suppress_depth += 1;
            tracer.trace_effect_suppressed(op); // Optional: for debugging
            return; // Don't emit to log
        }
        SuppressEnd => {
            self.suppress_depth = self.suppress_depth.saturating_sub(1);
            tracer.trace_effect_suppressed(op); // Optional: for debugging
            return; // Don't emit to log
        }
        _ => {}
    }
    
    // Skip all other effects when suppressing
    if self.suppress_depth > 0 {
        tracer.trace_effect_suppressed(op); // Optional: for debugging
        return;
    }
    
    // ... existing effect handling (Node, Text, Arr, etc.)
}
```

#### 5.4 Checkpoint Creation

Update all checkpoint creation sites to include suppress_depth:

```rust
self.checkpoints.push(Checkpoint {
    descendant_index: self.cursor.descendant_index(),
    effect_watermark: self.effects.len(),
    frame_index: self.frames.current(),
    recursion_depth: self.recursion_depth,
    ip: /* ... */,
    skip_policy: /* ... */,
    suppress_depth: self.suppress_depth,  // NEW
});
```

#### 5.5 Backtrack Restoration

Update `backtrack` to restore suppress_depth:

```rust
fn backtrack<T: Tracer>(&mut self, tracer: &mut T) -> Result<(), RuntimeError> {
    let cp = self.checkpoints.pop().ok_or(RuntimeError::NoMatch)?;
    tracer.trace_backtrack();
    self.cursor.goto_descendant(cp.descendant_index);
    self.effects.truncate(cp.effect_watermark);
    self.frames.restore(cp.frame_index);
    self.recursion_depth = cp.recursion_depth;
    self.suppress_depth = cp.suppress_depth;  // NEW
    
    // ... rest of backtrack logic
}
```

---

## Testing Strategy

### Parser Tests (`parser/lexer_tests.rs`, `parser/cst_tests.rs`)

```rust
#[test]
fn lex_suppressive_capture() {
    let tokens = lex("@_");
    assert_eq!(tokens[0].kind, SyntaxKind::SuppressiveCapture);
}

#[test]
fn lex_named_suppressive_capture() {
    let tokens = lex("@_foo_bar");
    assert_eq!(tokens[0].kind, SyntaxKind::SuppressiveCapture);
}

#[test]
fn parse_suppressive_capture() {
    let input = "(identifier) @_";
    let query = Query::expect_valid_ast(input).unwrap();
    // Verify Capture node exists and is_suppressive() returns true
}
```

### Type Inference Tests (`analyze/type_check/tests.rs`)

```rust
#[test]
fn suppressive_capture_not_in_type() {
    let input = "Q = (func (id) @_name)";
    let query = build_query(input);
    let type_info = query.type_context().get_def_type_by_name("Q");
    // Verify output type has no fields (or only non-suppressed ones)
}

#[test]
fn suppressive_capture_with_regular_sibling() {
    let input = "Q = (func (id) @_ (num) @num)";
    let query = build_query(input);
    // Verify output type has only `num` field
}
```

### Execution Tests (`engine/engine_tests.rs`)

```rust
#[test]
fn suppressive_capture_no_effects() {
    let query = "Q = (identifier) @_";
    let source = "foo";
    let effects = execute(query, source);
    // Verify effect log is empty (or has no Node/Set effects)
}

#[test]
fn suppressive_capture_nested() {
    let query = "Q = { (a) @_outer { (b) @_inner } (c) @visible }";
    let source = "a b c";
    let effects = execute(query, source);
    // Only @visible should produce effects
}

#[test]
fn suppressive_capture_backtrack() {
    let query = "Q = [(a) @_ (b) @visible]";
    let source = "b";
    let effects = execute(query, source);
    // First branch fails (suppressed), second succeeds
    // Verify suppress_depth correctly restored on backtrack
}
```

---

## Files to Modify

| File | Changes |
|------|---------|
| `parser/cst.rs` | Add `SuppressiveCapture` token |
| `parser/grammar/fields.rs` | Handle `SuppressiveCapture` in `try_parse_capture` |
| `parser/ast.rs` | Add `is_suppressive()` method to `CapturedExpr` |
| `analyze/type_check/infer.rs` | Skip suppressive captures in `infer_captured_expr` |
| `bytecode/effects.rs` | Add `SuppressBegin`, `SuppressEnd` opcodes |
| `compile/expressions.rs` | Emit suppress effects in `compile_captured_inner` |
| `engine/checkpoint.rs` | Add `suppress_depth` to `Checkpoint` |
| `engine/vm.rs` | Add `suppress_depth` to VM, handle in `emit_effect`/`backtrack` |
| `docs/lang-reference.md` | Document suppressive capture syntax |
| `docs/binary-format/06-transitions.md` | Document new effect opcodes |

---

## Edge Cases to Consider

1. **Bare `@_` without inner expression**: Should be a no-op (valid but useless)
2. **Type annotation on suppressive capture**: Should be a parse error (no `@_name :: Type`)
3. **Suppressive capture as row capture**: `{...}* @_` should suppress the entire array
4. **Nested regular inside suppressive**: `@_ { (a) @inner }` - `@inner` is suppressed too
5. **Suppressive inside regular**: `@outer { (a) @_ }` - `@outer` produces effects, inner is suppressed

---

## Implementation Order

1. **Lexer + Parser** - Get syntax working first
2. **AST helper** - `is_suppressive()` method  
3. **Type inference** - Skip suppressive in type computation
4. **Bytecode opcodes** - Add `SuppressBegin`/`SuppressEnd`
5. **Compilation** - Emit suppress effects around inner
6. **VM checkpoint** - Add `suppress_depth` field
7. **VM execution** - Handle suppress mode in `emit_effect`
8. **Tests** - Parser, type inference, execution
9. **Documentation** - Update lang-reference.md
