//! Regression: deep VM backtracking must not overflow the native stack.
//!
//! `VM::backtrack` once self-recursed in tail position over the checkpoint stack
//! (vm.rs). The checkpoint stack's depth is set by the *source-tree shape* and is
//! decoupled from call depth (the frame stack), so a single `backtrack` could
//! unwind a run of call-retry checkpoints far deeper than the frame stack ever
//! grew. Rust does not guarantee tail-call optimization, so on untrusted source
//! that recursion aborted the process on the native stack.
//!
//! The fix turned `backtrack` into a loop. This test pins it two ways:
//!   1. a probe `Tracer` asserts a single `backtrack` call unwinds a run of
//!      checkpoints far past any plausible native-stack depth, and
//!   2. the run happens on a deliberately tiny (256 KiB) thread stack, so the
//!      pre-fix recursive version would abort the test binary here.

use std::thread;

use arborium_tree_sitter::{Language as TsLanguage, Node, Parser as TsParser, Tree};
use indoc::indoc;

use crate::bytecode::{EffectKind, Instruction, Module, Nav};
use crate::compiler::test_utils::javascript_grammar;
use crate::core::{Colors, NodeFieldId};
use crate::{
    Limit, QueryBuilder, RuntimeEffect, RuntimeError, RuntimeLimitSpec, Tracer, VM, Value,
};

/// Number of nested `unary_expression`s the query descends through. Each level
/// leaves one call-retry checkpoint; the final failure unwinds all of them in a
/// single `backtrack` call. Chosen far above any native-stack frame budget: a
/// contiguous run of `DEPTH` recursive frames cannot fit in `STACK_SIZE` under any
/// realistic frame size, so the pre-fix recursion aborts the binary here. It is
/// not larger only because each checkpoint restore is O(tree-position) in
/// tree-sitter's cursor, making a full unwind O(DEPTH²) — 10k keeps the test a few
/// seconds while still being an order of magnitude past the stack's capacity.
const DEPTH: usize = 10_000;

/// Deliberately tiny: the pre-fix recursive `backtrack` would need ~`DEPTH` frames
/// and overflow this long before the leaf. The iterative version uses O(1) native
/// stack, so a constant handful of frames is all it needs.
const STACK_SIZE: usize = 256 * 1024;

/// A query whose match descends one `unary_expression` per level and then fails at
/// the bottom, forcing one maximally deep contiguous backtrack:
///
/// - `Top` anchors at the program root and descends to the unary chain. The root
///   preamble matches once (no tree-wide search), so the work stays linear in
///   `DEPTH` rather than quadratic.
/// - `Rec`'s escape branch matches `number` — a real expression, so the pattern is
///   matchable in principle (the grammar checker admits it) yet one that never
///   appears in a chain of `!` operators over an identifier. Trying it fails and
///   *consumes* the branch checkpoint at every level, so the descent leaves only
///   call-retry checkpoints — a contiguous run that backtrack pops without
///   re-entering. At the leaf `identifier`, both branches fail and the whole run
///   unwinds at once.
const QUERY: &str = indoc! {"
    Rec = [Leaf: (number) Deep: (unary_expression (Rec))]
    Top = (program (expression_statement (Rec)))
"};

/// Counts the longest run of consecutive `trace_backtrack` calls uninterrupted by
/// any other trace event — i.e. the deepest single `backtrack` unwind.
#[derive(Default)]
struct DepthProbe {
    run: u64,
    max_run: u64,
}

impl DepthProbe {
    /// A non-backtrack event ends the current run.
    fn boundary(&mut self) {
        self.run = 0;
    }
}

impl Tracer for DepthProbe {
    fn trace_backtrack(&mut self) {
        self.run += 1;
        self.max_run = self.max_run.max(self.run);
    }

    fn trace_instruction(&mut self, _ip: u16, _instr: &Instruction<'_>) {
        self.boundary();
    }
    fn trace_nav(&mut self, _nav: Nav, _node: Node<'_>) {
        self.boundary();
    }
    fn trace_nav_failure(&mut self, _nav: Nav) {
        self.boundary();
    }
    fn trace_match_success(&mut self, _node: Node<'_>) {
        self.boundary();
    }
    fn trace_match_failure(&mut self, _node: Node<'_>) {
        self.boundary();
    }
    fn trace_field_success(&mut self, _field_id: NodeFieldId) {
        self.boundary();
    }
    fn trace_field_failure(&mut self, _node: Node<'_>) {
        self.boundary();
    }
    fn trace_effect(&mut self, _effect: &RuntimeEffect<'_>) {
        self.boundary();
    }
    fn trace_effect_suppressed(&mut self, _opcode: EffectKind, _payload: usize) {
        self.boundary();
    }
    fn trace_suppress_control(&mut self, _opcode: EffectKind, _suppressed: bool) {
        self.boundary();
    }
    fn trace_call(&mut self, _target_ip: u16) {
        self.boundary();
    }
    fn trace_return(&mut self) {
        self.boundary();
    }
    fn trace_checkpoint_created(&mut self, _ip: u16) {
        self.boundary();
    }
    fn trace_enter_entrypoint(&mut self, _target_ip: u16) {
        self.boundary();
    }
}

fn compile(query: &str) -> Module {
    let compiled = QueryBuilder::from_inline(query)
        .compile(javascript_grammar())
        .expect("query compiles");
    assert!(
        compiled.is_valid(),
        "{}",
        compiled.diagnostics().render(compiled.source_map())
    );
    compiled.into_module().expect("query emits a module")
}

fn parse_js(source: &str) -> Tree {
    let mut parser = TsParser::new();
    let lang: TsLanguage = arborium_javascript::language().into();
    parser.set_language(&lang).expect("set javascript language");
    parser.parse(source, None).expect("parse source")
}

#[test]
fn deep_backtrack_does_not_overflow_native_stack() {
    let module = compile(QUERY);

    // A unary chain `!!!…!x` of DEPTH operators: DEPTH nested `unary_expression`s.
    let source = format!("{}x", "!".repeat(DEPTH));
    // Parse on the parent stack: tree-sitter parse/drop of a deep tree is its own
    // concern; this test isolates the VM's backtracking.
    let tree = parse_js(&source);

    let entry = module.entrypoint("Top").expect("Top is an entrypoint");

    // Run the VM on a tiny stack so the pre-fix recursive `backtrack` would abort
    // here. Both runtime limits are Unbounded so no resource ceiling cuts the run
    // short before the deep unwind (the frame stack lives on the heap); the native
    // stack is what's under test.
    let (outcome, max_run) = thread::scope(|scope| {
        let handle = thread::Builder::new()
            .name("deep-backtrack".into())
            .stack_size(STACK_SIZE)
            .spawn_scoped(scope, || {
                let vm = VM::builder(&source, &tree)
                    .limits(RuntimeLimitSpec {
                        steps: Limit::Unbounded,
                        memory: Limit::Unbounded,
                    })
                    .build();
                let mut probe = DepthProbe::default();
                let result = vm.execute_with(&module, &entry, &mut probe);
                let outcome = match result {
                    Ok(_) => "matched",
                    Err(RuntimeError::NoMatch) => "no-match",
                    Err(RuntimeError::StepLimitExceeded(_)) => "steps",
                    Err(RuntimeError::MemoryLimitExceeded { .. }) => "memory",
                    Err(_) => "other-error",
                };
                (outcome, probe.max_run)
            })
            .expect("spawn deep-backtrack thread");
        handle.join().expect("deep-backtrack thread did not abort")
    });

    // The query cannot match (no statement_block bottoms out the chain), so the run
    // unwinds fully to a clean no-match instead of crashing.
    assert_eq!(outcome, "no-match", "expected a clean no-match outcome");

    // One `backtrack` call must have unwound a contiguous run at least as deep as
    // the chain — proof the loop handled depth the old native recursion could not.
    assert!(
        max_run >= DEPTH as u64,
        "expected a single backtrack to unwind >= {DEPTH} checkpoints, saw {max_run}"
    );
}

/// A captured-recursive query materializes output as deep as the match, so both
/// rendering (`Value::format`) and dropping the value must avoid native recursion.
/// Build a deeply nested value, then render and drop it on a tiny stack: the
/// recursive printer and the derived recursive drop would each overflow here.
#[test]
fn deep_value_render_and_drop_do_not_overflow_native_stack() {
    // Far past any native-stack frame budget. Construction is bottom-up and linear,
    // and compact rendering is linear in depth, so a large depth stays cheap.
    const VALUE_DEPTH: usize = 200_000;

    let len = thread::scope(|scope| {
        let handle = thread::Builder::new()
            .name("deep-value".into())
            .stack_size(STACK_SIZE)
            .spawn_scoped(scope, || {
                // Bottom-up so construction itself never recurses.
                let mut value = Value::Null;
                for _ in 0..VALUE_DEPTH {
                    value = Value::Struct(vec![("inner".to_string(), value)]);
                }
                let rendered = value.format(false, Colors::new(false));
                // `value` drops here, on this same tiny stack.
                rendered.len()
            })
            .expect("spawn deep-value thread");
        handle.join().expect("deep-value thread did not abort")
    });

    // Each level contributes a fixed `{"inner":…}` wrapper, so output is linear in
    // depth and non-trivial — confirming the whole chain was rendered.
    assert!(
        len > VALUE_DEPTH,
        "expected rendered output longer than {VALUE_DEPTH}, saw {len}"
    );
}
