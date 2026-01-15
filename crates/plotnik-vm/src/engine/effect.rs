//! Runtime effects for VM execution.
//!
//! Runtime effects carry actual node references, unlike bytecode EffectOp
//! which only stores opcode + payload.

use arborium_tree_sitter::Node;

/// Runtime effect produced by VM execution.
///
/// Unlike bytecode `EffectOp`, runtime effects carry actual node references
/// for materialization. Lifetime `'t` denotes the parsed tree-sitter tree.
#[derive(Debug)]
pub enum RuntimeEffect<'t> {
    /// Capture a node reference.
    Node(Node<'t>),
    /// Extract source text from a node.
    Text(Node<'t>),
    /// Begin array scope.
    Arr,
    /// Push current value to array.
    Push,
    /// End array scope.
    EndArr,
    /// Begin object scope.
    Obj,
    /// Set field at member index.
    Set(u16),
    /// End object scope.
    EndObj,
    /// Begin enum variant at variant index.
    Enum(u16),
    /// End enum variant.
    EndEnum,
    /// Clear current value.
    Clear,
    /// Null placeholder (for optional/alternation).
    Null,
}

/// Effect log with truncation support for backtracking.
#[derive(Debug)]
pub struct EffectLog<'t>(Vec<RuntimeEffect<'t>>);

impl<'t> EffectLog<'t> {
    /// Create an empty effect log.
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Push an effect to the log.
    #[inline]
    pub fn push(&mut self, effect: RuntimeEffect<'t>) {
        self.0.push(effect);
    }

    /// Get current length (used as watermark for backtracking).
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Check if empty.
    #[inline]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Truncate to watermark (for backtracking).
    #[inline]
    pub fn truncate(&mut self, watermark: usize) {
        self.0.truncate(watermark);
    }

    /// Get effects as slice.
    pub fn as_slice(&self) -> &[RuntimeEffect<'t>] {
        &self.0
    }

    /// Consume into vec.
    #[allow(dead_code)]
    pub fn into_vec(self) -> Vec<RuntimeEffect<'t>> {
        self.0
    }
}

impl Default for EffectLog<'_> {
    fn default() -> Self {
        Self::new()
    }
}
