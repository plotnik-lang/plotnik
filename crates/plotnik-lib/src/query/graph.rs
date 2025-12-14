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

/// Array collection mode for loop combinators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayMode {
    /// No array collection (simple repetition)
    None,
    /// Collect elements into array (StartArray/PushElement/EndArray)
    Simple,
    /// Collect with object scope per iteration (for QIS)
    Qis,
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

    /// Generic loop combinator for * and + quantifiers.
    ///
    /// - `at_least_one`: true for + (one or more), false for * (zero or more)
    /// - `greedy`: true for greedy (try match first), false for lazy (try exit first)
    /// - `mode`: array collection mode
    fn build_repetition(
        &mut self,
        inner: Fragment,
        at_least_one: bool,
        greedy: bool,
        mode: ArrayMode,
        initial_nav: Nav,
    ) -> Fragment {
        let has_array = mode != ArrayMode::None;
        let has_qis = mode == ArrayMode::Qis;

        // Array wrapper nodes
        let start = if has_array {
            let s = self.add_epsilon();
            self.node_mut(s).add_effect(BuildEffect::StartArray {
                is_plus: at_least_one,
            });
            Some(s)
        } else {
            None
        };

        let end = if has_array {
            let e = self.add_epsilon();
            self.node_mut(e).add_effect(BuildEffect::EndArray);
            Some(e)
        } else {
            None
        };

        // QIS object wrapper nodes
        let (obj_start, obj_end) = if has_qis {
            let os = self.add_epsilon();
            self.node_mut(os).add_effect(BuildEffect::StartObject {
                for_alternation: false,
            });
            let oe = self.add_epsilon();
            self.node_mut(oe).add_effect(BuildEffect::EndObject);
            (Some(os), Some(oe))
        } else {
            (None, None)
        };

        // Push node for array modes
        let push = if has_array {
            let p = self.add_epsilon();
            self.node_mut(p).add_effect(BuildEffect::PushElement);
            Some(p)
        } else {
            None
        };

        // Branch node (decision point for loop continuation)
        let branch = self.add_epsilon();

        // Exit node for non-array modes
        let exit = if !has_array {
            Some(self.add_epsilon())
        } else {
            None
        };

        // Determine the effective inner entry/exit (with QIS wrapping if needed)
        let (loop_body_entry, loop_body_exit) = if has_qis {
            self.connect(obj_start.unwrap(), inner.entry);
            self.connect(inner.exit, obj_end.unwrap());
            (obj_start.unwrap(), obj_end.unwrap())
        } else {
            (inner.entry, inner.exit)
        };

        // Create first-entry node with initial navigation (e.g., Down for first child)
        let first_entry = self.add_epsilon();
        self.node_mut(first_entry).set_nav(initial_nav);
        self.connect(first_entry, loop_body_entry);

        // Create try_next node with Next navigation for subsequent iterations
        // This is separate from re_entry so that Next failure triggers backtracking
        // to re_entry's branch point, not to before the loop started
        let try_next = self.add_epsilon();
        self.node_mut(try_next).set_nav(Nav::next());
        self.connect(try_next, loop_body_entry);

        // Wire up the graph based on at_least_one and greedy
        if at_least_one {
            // + pattern: must match at least once
            // Entry → first_entry → body → push → re_entry → (try_next → body or exit)
            let entry_point = start.unwrap_or(first_entry);
            let exit_point = end.or(exit).unwrap();

            // re_entry is a branch point (no nav) that chooses: try more or exit
            let re_entry = self.add_epsilon();

            if let Some(s) = start {
                self.connect(s, first_entry);
            }

            if let Some(p) = push {
                self.connect(loop_body_exit, p);
                self.connect(p, re_entry);
            } else {
                self.connect(loop_body_exit, re_entry);
            }

            // re_entry branches: try_next (Next nav) or exit
            // If try_next's Next fails, backtrack finds re_entry checkpoint and tries exit
            if greedy {
                self.connect(re_entry, try_next);
                self.connect(re_entry, exit_point);
            } else {
                self.connect(re_entry, exit_point);
                self.connect(re_entry, try_next);
            }

            Fragment::new(entry_point, exit_point)
        } else {
            // * pattern: zero or more
            // Entry → branch → (first_entry → body → push → re_entry → try_next → body) or exit
            let entry_point = start.unwrap_or(branch);
            let exit_point = end.or(exit).unwrap();

            // re_entry is a branch point (no nav) that chooses: try more or exit
            let re_entry = self.add_epsilon();

            if let Some(s) = start {
                self.connect(s, branch);
            }

            if greedy {
                self.connect(branch, first_entry);
                self.connect(branch, exit_point);
            } else {
                self.connect(branch, exit_point);
                self.connect(branch, first_entry);
            }

            if let Some(p) = push {
                self.connect(loop_body_exit, p);
                self.connect(p, re_entry);
            } else {
                self.connect(loop_body_exit, re_entry);
            }

            // re_entry branches: try_next (Next nav) or exit
            // If try_next's Next fails, backtrack finds re_entry checkpoint and tries exit
            if greedy {
                self.connect(re_entry, try_next);
                self.connect(re_entry, exit_point);
            } else {
                self.connect(re_entry, exit_point);
                self.connect(re_entry, try_next);
            }

            Fragment::new(entry_point, exit_point)
        }
    }

    /// Generic optional combinator for ? quantifier.
    ///
    /// - `greedy`: true for greedy (try match first), false for lazy (try skip first)
    /// - `qis`: true to wrap the optional value in an object scope
    fn build_optional(&mut self, inner: Fragment, greedy: bool, qis: bool) -> Fragment {
        let branch = self.add_epsilon();
        let exit = self.add_epsilon();

        if qis {
            let obj_start = self.add_epsilon();
            self.node_mut(obj_start)
                .add_effect(BuildEffect::StartObject {
                    for_alternation: false,
                });

            let obj_end = self.add_epsilon();
            self.node_mut(obj_end).add_effect(BuildEffect::EndObject);

            // Skip path needs ClearCurrent to indicate "nothing captured"
            let skip = self.add_epsilon();
            self.node_mut(skip).add_effect(BuildEffect::ClearCurrent);

            self.connect(obj_start, inner.entry);
            self.connect(inner.exit, obj_end);
            self.connect(obj_end, exit);
            self.connect(skip, exit);

            if greedy {
                self.connect(branch, obj_start);
                self.connect(branch, skip);
            } else {
                self.connect(branch, skip);
                self.connect(branch, obj_start);
            }
        } else {
            let skip = self.add_epsilon();
            self.node_mut(skip).add_effect(BuildEffect::ClearCurrent);

            self.connect(skip, exit);
            self.connect(inner.exit, exit);

            if greedy {
                self.connect(branch, inner.entry);
                self.connect(branch, skip);
            } else {
                self.connect(branch, skip);
                self.connect(branch, inner.entry);
            }
        }

        Fragment::new(branch, exit)
    }

    /// Zero or more (greedy): inner*
    pub fn zero_or_more(&mut self, inner: Fragment, nav: Nav) -> Fragment {
        self.build_repetition(inner, false, true, ArrayMode::None, nav)
    }

    /// Zero or more (non-greedy): inner*?
    pub fn zero_or_more_lazy(&mut self, inner: Fragment, nav: Nav) -> Fragment {
        self.build_repetition(inner, false, false, ArrayMode::None, nav)
    }

    /// One or more (greedy): inner+
    pub fn one_or_more(&mut self, inner: Fragment, nav: Nav) -> Fragment {
        self.build_repetition(inner, true, true, ArrayMode::None, nav)
    }

    /// One or more (non-greedy): inner+?
    pub fn one_or_more_lazy(&mut self, inner: Fragment, nav: Nav) -> Fragment {
        self.build_repetition(inner, true, false, ArrayMode::None, nav)
    }

    /// Optional (greedy): inner?
    pub fn optional(&mut self, inner: Fragment) -> Fragment {
        self.build_optional(inner, true, false)
    }

    /// Optional (non-greedy): inner??
    pub fn optional_lazy(&mut self, inner: Fragment) -> Fragment {
        self.build_optional(inner, false, false)
    }

    /// Zero or more with array collection (greedy): inner*
    pub fn zero_or_more_array(&mut self, inner: Fragment, nav: Nav) -> Fragment {
        self.build_repetition(inner, false, true, ArrayMode::Simple, nav)
    }

    /// Zero or more with array collection (non-greedy): inner*?
    pub fn zero_or_more_array_lazy(&mut self, inner: Fragment, nav: Nav) -> Fragment {
        self.build_repetition(inner, false, false, ArrayMode::Simple, nav)
    }

    /// One or more with array collection (greedy): inner+
    pub fn one_or_more_array(&mut self, inner: Fragment, nav: Nav) -> Fragment {
        self.build_repetition(inner, true, true, ArrayMode::Simple, nav)
    }

    /// One or more with array collection (non-greedy): inner+?
    pub fn one_or_more_array_lazy(&mut self, inner: Fragment, nav: Nav) -> Fragment {
        self.build_repetition(inner, true, false, ArrayMode::Simple, nav)
    }

    /// Zero or more with QIS object wrapping (greedy): inner*
    ///
    /// Each iteration is wrapped in StartObject/EndObject to keep
    /// multiple captures coupled per-iteration.
    pub fn zero_or_more_array_qis(&mut self, inner: Fragment, nav: Nav) -> Fragment {
        self.build_repetition(inner, false, true, ArrayMode::Qis, nav)
    }

    /// Zero or more with QIS object wrapping (non-greedy): inner*?
    pub fn zero_or_more_array_qis_lazy(&mut self, inner: Fragment, nav: Nav) -> Fragment {
        self.build_repetition(inner, false, false, ArrayMode::Qis, nav)
    }

    /// One or more with QIS object wrapping (greedy): inner+
    pub fn one_or_more_array_qis(&mut self, inner: Fragment, nav: Nav) -> Fragment {
        self.build_repetition(inner, true, true, ArrayMode::Qis, nav)
    }

    /// One or more with QIS object wrapping (non-greedy): inner+?
    pub fn one_or_more_array_qis_lazy(&mut self, inner: Fragment, nav: Nav) -> Fragment {
        self.build_repetition(inner, true, false, ArrayMode::Qis, nav)
    }

    /// Optional with QIS object wrapping: inner?
    ///
    /// Wraps the optional value in an object scope.
    pub fn optional_qis(&mut self, inner: Fragment) -> Fragment {
        self.build_optional(inner, true, true)
    }

    /// Optional with QIS object wrapping (non-greedy): inner??
    pub fn optional_qis_lazy(&mut self, inner: Fragment) -> Fragment {
        self.build_optional(inner, false, true)
    }

    /// Wrap definitions that don't already match the root node kind.
    ///
    /// For each definition whose entry matcher doesn't match `root_kind`,
    /// prepends a transition that matches the root and descends into children.
    /// This allows queries like `(function_declaration)` to work when the
    /// interpreter starts at tree root (e.g., `program`).
    pub fn wrap_definitions_with_root(&mut self, root_kind: &'src str) {
        let def_names: Vec<&'src str> = self.definitions.keys().copied().collect();

        for name in def_names {
            let entry = self.definitions[name];

            // Check if entry already matches root (directly or first reachable matcher)
            if self.entry_matches_root(entry, root_kind) {
                continue;
            }

            // Create wrapper: (root_kind) with Nav::stay
            let wrapper = self.add_node(BuildNode::with_matcher(BuildMatcher::node(root_kind)));

            // Add epsilon node with Nav::down between wrapper and original entry
            let down_nav = self.add_epsilon();
            self.node_mut(down_nav).set_nav(Nav::down());

            // Connect wrapper → down_nav → original entry
            self.connect(wrapper, down_nav);
            self.connect(down_nav, entry);

            // Update definition to point to wrapper
            self.definitions.insert(name, wrapper);
        }
    }

    /// Check if entry (or first reachable node matcher) already matches root kind.
    fn entry_matches_root(&self, entry: NodeId, root_kind: &str) -> bool {
        match &self.nodes[entry as usize].matcher {
            BuildMatcher::Node { kind, .. } => *kind == root_kind,
            BuildMatcher::Epsilon => {
                // For epsilon entries, check first reachable node matchers
                for &target in &self.nodes[entry as usize].successors {
                    if self.entry_matches_root(target, root_kind) {
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
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
    /// Clear current value (set to None). Used on skip paths for optional captures.
    ClearCurrent,
    /// Start array collection. `is_plus` distinguishes `+` (true) from `*` (false).
    StartArray {
        is_plus: bool,
    },
    PushElement,
    EndArray,
    /// Start object scope. `for_alternation` is true when this object wraps a captured
    /// tagged alternation (tags should create enum), false for QIS/sequence objects
    /// (tags in inner alternations should be ignored).
    StartObject {
        for_alternation: bool,
    },
    EndObject,
    Field {
        name: &'src str,
        span: TextRange,
    },
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

    pub fn is_some(&self) -> bool {
        !matches!(self, RefMarker::None)
    }

    pub fn is_enter(&self) -> bool {
        matches!(self, RefMarker::Enter { .. })
    }

    pub fn is_exit(&self) -> bool {
        matches!(self, RefMarker::Exit { .. })
    }
}
