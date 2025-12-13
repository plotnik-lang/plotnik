//! Core types for build-time query graphs.
//!
//! The graph uses index-based node references (`NodeId`) with nodes stored
//! in a `Vec`. Strings borrow from the source (`&'src str`) until IR emission.

use crate::ir::Nav;
use indexmap::IndexMap;
use rowan::TextRange;

/// Index into `BuildGraph::nodes`.
pub type NodeId = u32;

/// A graph fragment with single entry and exit points.
///
/// Every expression compiles to a fragment. Combinators connect fragments
/// by manipulating entry/exit edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fragment {
    pub entry: NodeId,
    pub exit: NodeId,
}

impl Fragment {
    pub fn new(entry: NodeId, exit: NodeId) -> Self {
        Self { entry, exit }
    }

    pub fn single(node: NodeId) -> Self {
        Self {
            entry: node,
            exit: node,
        }
    }
}

/// Build-time graph for query compilation.
///
/// Nodes are stored in a flat vector, referenced by `NodeId`.
/// Definitions map names to their entry points.
#[derive(Debug)]
pub struct BuildGraph<'src> {
    nodes: Vec<BuildNode<'src>>,
    definitions: IndexMap<&'src str, NodeId>,
}

impl<'src> BuildGraph<'src> {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            definitions: IndexMap::new(),
        }
    }

    pub fn add_node(&mut self, node: BuildNode<'src>) -> NodeId {
        let id = self.nodes.len() as NodeId;
        self.nodes.push(node);
        id
    }

    pub fn add_epsilon(&mut self) -> NodeId {
        self.add_node(BuildNode::epsilon())
    }

    pub fn add_matcher(&mut self, matcher: BuildMatcher<'src>) -> NodeId {
        self.add_node(BuildNode::with_matcher(matcher))
    }

    pub fn add_definition(&mut self, name: &'src str, entry: NodeId) {
        self.definitions.insert(name, entry);
    }

    pub fn definition(&self, name: &str) -> Option<NodeId> {
        self.definitions.get(name).copied()
    }

    pub fn definitions(&self) -> impl Iterator<Item = (&'src str, NodeId)> + '_ {
        self.definitions.iter().map(|(k, v)| (*k, *v))
    }

    pub fn node(&self, id: NodeId) -> &BuildNode<'src> {
        &self.nodes[id as usize]
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut BuildNode<'src> {
        &mut self.nodes[id as usize]
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (NodeId, &BuildNode<'src>)> {
        self.nodes.iter().enumerate().map(|(i, n)| (i as NodeId, n))
    }

    pub fn connect(&mut self, from: NodeId, to: NodeId) {
        self.nodes[from as usize].successors.push(to);
    }

    pub fn connect_exit(&mut self, fragment: Fragment, to: NodeId) {
        self.connect(fragment.exit, to);
    }

    // ─────────────────────────────────────────────────────────────────────
    // Fragment Combinators
    // ─────────────────────────────────────────────────────────────────────

    pub fn matcher_fragment(&mut self, matcher: BuildMatcher<'src>) -> Fragment {
        Fragment::single(self.add_matcher(matcher))
    }

    pub fn epsilon_fragment(&mut self) -> Fragment {
        Fragment::single(self.add_epsilon())
    }

    /// Connect fragments in sequence: f1 → f2 → ... → fn
    pub fn sequence(&mut self, fragments: &[Fragment]) -> Fragment {
        match fragments.len() {
            0 => self.epsilon_fragment(),
            1 => fragments[0],
            _ => {
                for window in fragments.windows(2) {
                    self.connect(window[0].exit, window[1].entry);
                }
                Fragment::new(fragments[0].entry, fragments[fragments.len() - 1].exit)
            }
        }
    }

    /// Connect fragments in parallel (alternation): entry → [f1|f2|...|fn] → exit
    pub fn alternation(&mut self, fragments: &[Fragment]) -> Fragment {
        if fragments.is_empty() {
            return self.epsilon_fragment();
        }
        if fragments.len() == 1 {
            return fragments[0];
        }

        let entry = self.add_epsilon();
        let exit = self.add_epsilon();

        for f in fragments {
            self.connect(entry, f.entry);
            self.connect(f.exit, exit);
        }

        Fragment::new(entry, exit)
    }

    /// Zero or more (greedy): inner*
    pub fn zero_or_more(&mut self, inner: Fragment) -> Fragment {
        let branch = self.add_epsilon();
        let exit = self.add_epsilon();

        self.connect(branch, inner.entry);
        self.connect(branch, exit);
        self.connect(inner.exit, branch);

        Fragment::new(branch, exit)
    }

    /// Zero or more (non-greedy): inner*?
    pub fn zero_or_more_lazy(&mut self, inner: Fragment) -> Fragment {
        let branch = self.add_epsilon();
        let exit = self.add_epsilon();

        self.connect(branch, exit);
        self.connect(branch, inner.entry);
        self.connect(inner.exit, branch);

        Fragment::new(branch, exit)
    }

    /// One or more (greedy): inner+
    pub fn one_or_more(&mut self, inner: Fragment) -> Fragment {
        let branch = self.add_epsilon();
        let exit = self.add_epsilon();

        self.connect(inner.exit, branch);
        self.connect(branch, inner.entry);
        self.connect(branch, exit);

        Fragment::new(inner.entry, exit)
    }

    /// One or more (non-greedy): inner+?
    pub fn one_or_more_lazy(&mut self, inner: Fragment) -> Fragment {
        let branch = self.add_epsilon();
        let exit = self.add_epsilon();

        self.connect(inner.exit, branch);
        self.connect(branch, exit);
        self.connect(branch, inner.entry);

        Fragment::new(inner.entry, exit)
    }

    /// Optional (greedy): inner?
    pub fn optional(&mut self, inner: Fragment) -> Fragment {
        let branch = self.add_epsilon();
        let exit = self.add_epsilon();

        self.connect(branch, inner.entry);
        self.connect(branch, exit);
        self.connect(inner.exit, exit);

        Fragment::new(branch, exit)
    }

    /// Optional (non-greedy): inner??
    pub fn optional_lazy(&mut self, inner: Fragment) -> Fragment {
        let branch = self.add_epsilon();
        let exit = self.add_epsilon();

        self.connect(branch, exit);
        self.connect(branch, inner.entry);
        self.connect(inner.exit, exit);

        Fragment::new(branch, exit)
    }

    // ─────────────────────────────────────────────────────────────────────
    // Array-Collecting Loop Combinators
    // ─────────────────────────────────────────────────────────────────────

    /// Zero or more with array collection (greedy): inner*
    pub fn zero_or_more_array(&mut self, inner: Fragment) -> Fragment {
        let start = self.add_epsilon();
        self.node_mut(start).add_effect(BuildEffect::StartArray);

        let branch = self.add_epsilon();
        let push = self.add_epsilon();
        self.node_mut(push).add_effect(BuildEffect::PushElement);

        let end = self.add_epsilon();
        self.node_mut(end).add_effect(BuildEffect::EndArray);

        self.connect(start, branch);
        self.connect(branch, inner.entry);
        self.connect(branch, end);
        self.connect(inner.exit, push);
        self.connect(push, branch);

        Fragment::new(start, end)
    }

    /// Zero or more with array collection (non-greedy): inner*?
    pub fn zero_or_more_array_lazy(&mut self, inner: Fragment) -> Fragment {
        let start = self.add_epsilon();
        self.node_mut(start).add_effect(BuildEffect::StartArray);

        let branch = self.add_epsilon();
        let push = self.add_epsilon();
        self.node_mut(push).add_effect(BuildEffect::PushElement);

        let end = self.add_epsilon();
        self.node_mut(end).add_effect(BuildEffect::EndArray);

        self.connect(start, branch);
        self.connect(branch, end);
        self.connect(branch, inner.entry);
        self.connect(inner.exit, push);
        self.connect(push, branch);

        Fragment::new(start, end)
    }

    /// One or more with array collection (greedy): inner+
    pub fn one_or_more_array(&mut self, inner: Fragment) -> Fragment {
        let start = self.add_epsilon();
        self.node_mut(start).add_effect(BuildEffect::StartArray);

        let push = self.add_epsilon();
        self.node_mut(push).add_effect(BuildEffect::PushElement);

        let branch = self.add_epsilon();

        let end = self.add_epsilon();
        self.node_mut(end).add_effect(BuildEffect::EndArray);

        self.connect(start, inner.entry);
        self.connect(inner.exit, push);
        self.connect(push, branch);
        self.connect(branch, inner.entry);
        self.connect(branch, end);

        Fragment::new(start, end)
    }

    /// One or more with array collection (non-greedy): inner+?
    pub fn one_or_more_array_lazy(&mut self, inner: Fragment) -> Fragment {
        let start = self.add_epsilon();
        self.node_mut(start).add_effect(BuildEffect::StartArray);

        let push = self.add_epsilon();
        self.node_mut(push).add_effect(BuildEffect::PushElement);

        let branch = self.add_epsilon();

        let end = self.add_epsilon();
        self.node_mut(end).add_effect(BuildEffect::EndArray);

        self.connect(start, inner.entry);
        self.connect(inner.exit, push);
        self.connect(push, branch);
        self.connect(branch, end);
        self.connect(branch, inner.entry);

        Fragment::new(start, end)
    }

    // ─────────────────────────────────────────────────────────────────────
    // QIS-Aware Array Combinators (wrap each iteration with object scope)
    // ─────────────────────────────────────────────────────────────────────

    /// Zero or more with QIS object wrapping (greedy): inner*
    ///
    /// Each iteration is wrapped in StartObject/EndObject to keep
    /// multiple captures coupled per-iteration.
    pub fn zero_or_more_array_qis(&mut self, inner: Fragment) -> Fragment {
        let start = self.add_epsilon();
        self.node_mut(start).add_effect(BuildEffect::StartArray);

        let branch = self.add_epsilon();

        let obj_start = self.add_epsilon();
        self.node_mut(obj_start)
            .add_effect(BuildEffect::StartObject);

        let obj_end = self.add_epsilon();
        self.node_mut(obj_end).add_effect(BuildEffect::EndObject);

        let push = self.add_epsilon();
        self.node_mut(push).add_effect(BuildEffect::PushElement);

        let end = self.add_epsilon();
        self.node_mut(end).add_effect(BuildEffect::EndArray);

        self.connect(start, branch);
        self.connect(branch, obj_start);
        self.connect(branch, end);
        self.connect(obj_start, inner.entry);
        self.connect(inner.exit, obj_end);
        self.connect(obj_end, push);
        self.connect(push, branch);

        Fragment::new(start, end)
    }

    /// Zero or more with QIS object wrapping (non-greedy): inner*?
    pub fn zero_or_more_array_qis_lazy(&mut self, inner: Fragment) -> Fragment {
        let start = self.add_epsilon();
        self.node_mut(start).add_effect(BuildEffect::StartArray);

        let branch = self.add_epsilon();

        let obj_start = self.add_epsilon();
        self.node_mut(obj_start)
            .add_effect(BuildEffect::StartObject);

        let obj_end = self.add_epsilon();
        self.node_mut(obj_end).add_effect(BuildEffect::EndObject);

        let push = self.add_epsilon();
        self.node_mut(push).add_effect(BuildEffect::PushElement);

        let end = self.add_epsilon();
        self.node_mut(end).add_effect(BuildEffect::EndArray);

        self.connect(start, branch);
        self.connect(branch, end);
        self.connect(branch, obj_start);
        self.connect(obj_start, inner.entry);
        self.connect(inner.exit, obj_end);
        self.connect(obj_end, push);
        self.connect(push, branch);

        Fragment::new(start, end)
    }

    /// One or more with QIS object wrapping (greedy): inner+
    pub fn one_or_more_array_qis(&mut self, inner: Fragment) -> Fragment {
        let start = self.add_epsilon();
        self.node_mut(start).add_effect(BuildEffect::StartArray);

        let obj_start = self.add_epsilon();
        self.node_mut(obj_start)
            .add_effect(BuildEffect::StartObject);

        let obj_end = self.add_epsilon();
        self.node_mut(obj_end).add_effect(BuildEffect::EndObject);

        let push = self.add_epsilon();
        self.node_mut(push).add_effect(BuildEffect::PushElement);

        let branch = self.add_epsilon();

        let end = self.add_epsilon();
        self.node_mut(end).add_effect(BuildEffect::EndArray);

        self.connect(start, obj_start);
        self.connect(obj_start, inner.entry);
        self.connect(inner.exit, obj_end);
        self.connect(obj_end, push);
        self.connect(push, branch);
        self.connect(branch, obj_start);
        self.connect(branch, end);

        Fragment::new(start, end)
    }

    /// One or more with QIS object wrapping (non-greedy): inner+?
    pub fn one_or_more_array_qis_lazy(&mut self, inner: Fragment) -> Fragment {
        let start = self.add_epsilon();
        self.node_mut(start).add_effect(BuildEffect::StartArray);

        let obj_start = self.add_epsilon();
        self.node_mut(obj_start)
            .add_effect(BuildEffect::StartObject);

        let obj_end = self.add_epsilon();
        self.node_mut(obj_end).add_effect(BuildEffect::EndObject);

        let push = self.add_epsilon();
        self.node_mut(push).add_effect(BuildEffect::PushElement);

        let branch = self.add_epsilon();

        let end = self.add_epsilon();
        self.node_mut(end).add_effect(BuildEffect::EndArray);

        self.connect(start, obj_start);
        self.connect(obj_start, inner.entry);
        self.connect(inner.exit, obj_end);
        self.connect(obj_end, push);
        self.connect(push, branch);
        self.connect(branch, end);
        self.connect(branch, obj_start);

        Fragment::new(start, end)
    }

    /// Optional with QIS object wrapping: inner?
    ///
    /// Wraps the optional value in an object scope.
    pub fn optional_qis(&mut self, inner: Fragment) -> Fragment {
        let branch = self.add_epsilon();

        let obj_start = self.add_epsilon();
        self.node_mut(obj_start)
            .add_effect(BuildEffect::StartObject);

        let obj_end = self.add_epsilon();
        self.node_mut(obj_end).add_effect(BuildEffect::EndObject);

        let exit = self.add_epsilon();

        self.connect(branch, obj_start);
        self.connect(branch, exit);
        self.connect(obj_start, inner.entry);
        self.connect(inner.exit, obj_end);
        self.connect(obj_end, exit);

        Fragment::new(branch, exit)
    }

    /// Optional with QIS object wrapping (non-greedy): inner??
    pub fn optional_qis_lazy(&mut self, inner: Fragment) -> Fragment {
        let branch = self.add_epsilon();

        let obj_start = self.add_epsilon();
        self.node_mut(obj_start)
            .add_effect(BuildEffect::StartObject);

        let obj_end = self.add_epsilon();
        self.node_mut(obj_end).add_effect(BuildEffect::EndObject);

        let exit = self.add_epsilon();

        self.connect(branch, exit);
        self.connect(branch, obj_start);
        self.connect(obj_start, inner.entry);
        self.connect(inner.exit, obj_end);
        self.connect(obj_end, exit);

        Fragment::new(branch, exit)
    }
}

impl Default for BuildGraph<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// A node in the build graph.
#[derive(Debug, Clone)]
pub struct BuildNode<'src> {
    pub matcher: BuildMatcher<'src>,
    pub effects: Vec<BuildEffect<'src>>,
    pub ref_marker: RefMarker,
    pub successors: Vec<NodeId>,
    pub nav: Nav,
    pub ref_name: Option<&'src str>,
}

impl<'src> BuildNode<'src> {
    pub fn epsilon() -> Self {
        Self {
            matcher: BuildMatcher::Epsilon,
            effects: Vec::new(),
            ref_marker: RefMarker::None,
            successors: Vec::new(),
            nav: Nav::stay(),
            ref_name: None,
        }
    }

    pub fn with_matcher(matcher: BuildMatcher<'src>) -> Self {
        Self {
            matcher,
            effects: Vec::new(),
            ref_marker: RefMarker::None,
            successors: Vec::new(),
            nav: Nav::stay(),
            ref_name: None,
        }
    }

    pub fn add_effect(&mut self, effect: BuildEffect<'src>) {
        self.effects.push(effect);
    }

    pub fn set_ref_marker(&mut self, marker: RefMarker) {
        self.ref_marker = marker;
    }

    pub fn set_nav(&mut self, nav: Nav) {
        self.nav = nav;
    }

    pub fn is_epsilon(&self) -> bool {
        matches!(self.matcher, BuildMatcher::Epsilon)
    }
}

/// What a transition matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildMatcher<'src> {
    Epsilon,
    Node {
        kind: &'src str,
        field: Option<&'src str>,
        negated_fields: Vec<&'src str>,
    },
    Anonymous {
        literal: &'src str,
        field: Option<&'src str>,
    },
    Wildcard {
        field: Option<&'src str>,
    },
}

impl<'src> BuildMatcher<'src> {
    pub fn node(kind: &'src str) -> Self {
        Self::Node {
            kind,
            field: None,
            negated_fields: Vec::new(),
        }
    }

    pub fn anonymous(literal: &'src str) -> Self {
        Self::Anonymous {
            literal,
            field: None,
        }
    }

    pub fn wildcard() -> Self {
        Self::Wildcard { field: None }
    }

    pub fn with_field(mut self, field: &'src str) -> Self {
        match &mut self {
            BuildMatcher::Node { field: f, .. } => *f = Some(field),
            BuildMatcher::Anonymous { field: f, .. } => *f = Some(field),
            BuildMatcher::Wildcard { field: f } => *f = Some(field),
            BuildMatcher::Epsilon => {}
        }
        self
    }

    pub fn with_negated_field(mut self, field: &'src str) -> Self {
        if let BuildMatcher::Node { negated_fields, .. } = &mut self {
            negated_fields.push(field);
        }
        self
    }
}

/// Effect operations recorded during graph construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildEffect<'src> {
    CaptureNode,
    StartArray,
    PushElement,
    EndArray,
    StartObject,
    EndObject,
    Field { name: &'src str, span: TextRange },
    StartVariant(&'src str),
    EndVariant,
    ToString,
}

/// Marker for definition call/return transitions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RefMarker {
    #[default]
    None,
    Enter {
        ref_id: u32,
    },
    Exit {
        ref_id: u32,
    },
}

impl RefMarker {
    pub fn enter(ref_id: u32) -> Self {
        Self::Enter { ref_id }
    }

    pub fn exit(ref_id: u32) -> Self {
        Self::Exit { ref_id }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, RefMarker::None)
    }

    pub fn is_enter(&self) -> bool {
        matches!(self, RefMarker::Enter { .. })
    }

    pub fn is_exit(&self) -> bool {
        matches!(self, RefMarker::Exit { .. })
    }
}
