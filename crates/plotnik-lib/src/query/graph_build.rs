//! Graph construction integrated with Query pipeline.
//!
//! Constructs a `BuildGraph` from the parsed AST, reusing the `symbol_table`
//! and `qis_triggers` populated by earlier passes.

use std::collections::HashSet;

use crate::ir::Nav;
use crate::parser::{
    AltExpr, AltKind, AnonymousNode, Branch, CapturedExpr, Expr, FieldExpr, NamedNode,
    NegatedField, QuantifiedExpr, Ref, SeqExpr, SeqItem, SyntaxKind, token_src,
};

use super::Query;
use super::graph::{BuildEffect, BuildMatcher, Fragment, NodeId, RefMarker};

/// Context for navigation determination.
/// When `anchored` is true, `prev_anonymous` indicates whether the preceding
/// expression was anonymous (string literal), which determines Exact vs SkipTrivia mode.
#[derive(Debug, Clone, Copy)]
enum NavContext {
    Root,
    FirstChild {
        anchored: bool,
        prev_anonymous: bool,
    },
    Sibling {
        anchored: bool,
        prev_anonymous: bool,
    },
}

impl NavContext {
    fn to_nav(self) -> Nav {
        match self {
            NavContext::Root => Nav::stay(),
            NavContext::FirstChild {
                anchored: false, ..
            } => Nav::down(),
            NavContext::FirstChild {
                anchored: true,
                prev_anonymous,
            } => {
                if prev_anonymous {
                    Nav::down_exact()
                } else {
                    Nav::down_skip_trivia()
                }
            }
            NavContext::Sibling {
                anchored: false, ..
            } => Nav::next(),
            NavContext::Sibling {
                anchored: true,
                prev_anonymous,
            } => {
                if prev_anonymous {
                    Nav::next_exact()
                } else {
                    Nav::next_skip_trivia()
                }
            }
        }
    }
}

/// Tracks trailing anchor state for Up navigation.
#[derive(Debug, Clone, Copy)]
struct ExitContext {
    has_trailing_anchor: bool,
    last_was_anonymous: bool,
}

impl ExitContext {
    fn to_up_nav(self, level: u8) -> Nav {
        if !self.has_trailing_anchor {
            Nav::up(level)
        } else if self.last_was_anonymous {
            Nav::up_exact(level)
        } else {
            Nav::up_skip_trivia(level)
        }
    }
}

impl<'a> Query<'a> {
    /// Build the graph from the already-populated symbol_table.
    ///
    /// This method reuses the symbol_table from name resolution and
    /// qis_triggers from QIS detection.
    pub(super) fn construct_graph(&mut self) {
        self.next_ref_id = 0;

        let entries: Vec<_> = self
            .symbol_table
            .iter()
            .map(|(name, body)| (*name, body.clone()))
            .collect();
        for (name, body) in entries {
            let fragment = self.construct_expr(&body, NavContext::Root);
            self.graph.add_definition(name, fragment.entry);
        }

        self.link_references();
    }

    /// Link Enter nodes to their definition entry points.
    fn link_references(&mut self) {
        let mut links: Vec<(NodeId, &'a str, Vec<NodeId>)> = Vec::new();

        for (id, node) in self.graph.iter() {
            if let RefMarker::Enter { .. } = &node.ref_marker {
                if let Some(name) = node.ref_name {
                    let exit_successors = self.find_exit_successors_for_enter(id);
                    links.push((id, name, exit_successors));
                }
            }
        }

        for (enter_id, name, return_transitions) in links {
            if let Some(def_entry) = self.graph.definition(name) {
                self.graph.connect(enter_id, def_entry);
                for ret in return_transitions {
                    self.graph.connect(enter_id, ret);
                }
            }
        }
    }

    fn find_exit_successors_for_enter(&self, enter_id: NodeId) -> Vec<NodeId> {
        let enter_node = self.graph.node(enter_id);
        let RefMarker::Enter { ref_id } = enter_node.ref_marker else {
            return Vec::new();
        };

        for (_, node) in self.graph.iter() {
            if let RefMarker::Exit { ref_id: exit_id } = &node.ref_marker {
                if *exit_id == ref_id {
                    return node.successors.clone();
                }
            }
        }
        Vec::new()
    }

    fn construct_expr(&mut self, expr: &Expr, ctx: NavContext) -> Fragment {
        match expr {
            Expr::NamedNode(node) => self.construct_named_node(node, ctx),
            Expr::AnonymousNode(node) => self.construct_anonymous_node(node, ctx),
            Expr::Ref(r) => self.construct_ref(r, ctx),
            Expr::AltExpr(alt) => self.construct_alt(alt, ctx),
            Expr::SeqExpr(seq) => self.construct_seq(seq, ctx),
            Expr::CapturedExpr(cap) => self.construct_capture(cap, ctx),
            Expr::QuantifiedExpr(quant) => self.construct_quantifier(quant, ctx),
            Expr::FieldExpr(field) => self.construct_field(field, ctx),
        }
    }

    fn construct_named_node(&mut self, node: &NamedNode, ctx: NavContext) -> Fragment {
        let matcher = self.build_named_matcher(node);
        let nav = ctx.to_nav();
        let node_id = self.graph.add_matcher(matcher);
        self.graph.node_mut(node_id).set_nav(nav);

        let items: Vec<_> = node.items().collect();
        if items.is_empty() {
            return Fragment::single(node_id);
        }

        let (child_fragments, exit_ctx) = self.construct_item_sequence(&items, true);
        if child_fragments.is_empty() {
            return Fragment::single(node_id);
        }

        let inner = self.graph.sequence(&child_fragments);
        self.graph.connect(node_id, inner.entry);

        let exit_id = self.graph.add_epsilon();
        self.graph.node_mut(exit_id).set_nav(exit_ctx.to_up_nav(1));
        self.graph.connect(inner.exit, exit_id);

        Fragment::new(node_id, exit_id)
    }

    fn construct_item_sequence(
        &mut self,
        items: &[SeqItem],
        is_children: bool,
    ) -> (Vec<Fragment>, ExitContext) {
        let mut fragments = Vec::new();
        let mut pending_anchor = false;
        let mut last_was_anonymous = false;
        let mut is_first = true;

        for item in items {
            match item {
                SeqItem::Anchor(_) => {
                    pending_anchor = true;
                }
                SeqItem::Expr(expr) => {
                    let ctx = if is_first {
                        is_first = false;
                        if is_children {
                            NavContext::FirstChild {
                                anchored: pending_anchor,
                                prev_anonymous: last_was_anonymous,
                            }
                        } else {
                            NavContext::Sibling {
                                anchored: pending_anchor,
                                prev_anonymous: last_was_anonymous,
                            }
                        }
                    } else {
                        NavContext::Sibling {
                            anchored: pending_anchor,
                            prev_anonymous: last_was_anonymous,
                        }
                    };

                    last_was_anonymous = is_anonymous_expr(expr);
                    let frag = self.construct_expr(expr, ctx);
                    fragments.push(frag);
                    pending_anchor = false;
                }
            }
        }

        let exit_ctx = ExitContext {
            has_trailing_anchor: pending_anchor,
            last_was_anonymous,
        };

        (fragments, exit_ctx)
    }

    fn build_named_matcher(&self, node: &NamedNode) -> BuildMatcher<'a> {
        let kind = node
            .node_type()
            .map(|t| token_src(&t, self.source))
            .unwrap_or("_");

        let negated_fields: Vec<&'a str> = node
            .as_cst()
            .children()
            .filter_map(NegatedField::cast)
            .filter_map(|nf| nf.name())
            .map(|t| token_src(&t, self.source))
            .collect();

        let field = self.find_field_constraint(node.as_cst());

        if node.is_any() {
            BuildMatcher::Wildcard { field }
        } else {
            BuildMatcher::Node {
                kind,
                field,
                negated_fields,
            }
        }
    }

    fn construct_anonymous_node(&mut self, node: &AnonymousNode, ctx: NavContext) -> Fragment {
        let field = self.find_field_constraint(node.as_cst());
        let nav = ctx.to_nav();

        let matcher = if node.is_any() {
            BuildMatcher::Wildcard { field }
        } else {
            let literal = node
                .value()
                .map(|t| token_src(&t, self.source))
                .unwrap_or("");
            BuildMatcher::Anonymous { literal, field }
        };

        let node_id = self.graph.add_matcher(matcher);
        self.graph.node_mut(node_id).set_nav(nav);
        Fragment::single(node_id)
    }

    fn construct_ref(&mut self, r: &Ref, ctx: NavContext) -> Fragment {
        let Some(name_token) = r.name() else {
            return self.graph.epsilon_fragment();
        };

        let ref_id = self.next_ref_id;
        self.next_ref_id += 1;

        let enter_id = self.graph.add_epsilon();
        let nav = ctx.to_nav();
        self.graph.node_mut(enter_id).set_nav(nav);
        self.graph
            .node_mut(enter_id)
            .set_ref_marker(RefMarker::enter(ref_id));

        let exit_id = self.graph.add_epsilon();
        self.graph
            .node_mut(exit_id)
            .set_ref_marker(RefMarker::exit(ref_id));

        let name = token_src(&name_token, self.source);
        self.graph.node_mut(enter_id).ref_name = Some(name);

        Fragment::new(enter_id, exit_id)
    }

    fn construct_alt(&mut self, alt: &AltExpr, ctx: NavContext) -> Fragment {
        match alt.kind() {
            AltKind::Tagged => self.construct_tagged_alt(alt, ctx),
            AltKind::Untagged | AltKind::Mixed => self.construct_untagged_alt(alt, ctx),
        }
    }

    fn construct_tagged_alt(&mut self, alt: &AltExpr, ctx: NavContext) -> Fragment {
        let branches: Vec<_> = alt.branches().collect();
        if branches.is_empty() {
            return self.graph.epsilon_fragment();
        }

        let branch_id = self.graph.add_epsilon();
        self.graph.node_mut(branch_id).set_nav(ctx.to_nav());

        let exit_id = self.graph.add_epsilon();

        for branch in &branches {
            let frag = self.construct_tagged_branch(branch);
            self.graph.connect(branch_id, frag.entry);
            self.graph.connect(frag.exit, exit_id);
        }

        Fragment::new(branch_id, exit_id)
    }

    fn construct_tagged_branch(&mut self, branch: &Branch) -> Fragment {
        let Some(label_token) = branch.label() else {
            return branch
                .body()
                .map(|b| self.construct_expr(&b, NavContext::Root))
                .unwrap_or_else(|| self.graph.epsilon_fragment());
        };
        let Some(body) = branch.body() else {
            return self.graph.epsilon_fragment();
        };

        let label = token_src(&label_token, self.source);

        let start_id = self.graph.add_epsilon();
        self.graph
            .node_mut(start_id)
            .add_effect(BuildEffect::StartVariant(label));

        let body_frag = self.construct_expr(&body, NavContext::Root);

        let end_id = self.graph.add_epsilon();
        self.graph
            .node_mut(end_id)
            .add_effect(BuildEffect::EndVariant);

        self.graph.connect(start_id, body_frag.entry);
        self.graph.connect(body_frag.exit, end_id);

        Fragment::new(start_id, end_id)
    }

    fn construct_untagged_alt(&mut self, alt: &AltExpr, ctx: NavContext) -> Fragment {
        let branches: Vec<_> = alt.branches().filter_map(|b| b.body()).collect();

        if branches.is_empty() {
            return self.graph.epsilon_fragment();
        }

        let branch_id = self.graph.add_epsilon();
        self.graph.node_mut(branch_id).set_nav(ctx.to_nav());

        let exit_id = self.graph.add_epsilon();

        for body in &branches {
            let frag = self.construct_expr(body, NavContext::Root);
            self.graph.connect(branch_id, frag.entry);
            self.graph.connect(frag.exit, exit_id);
        }

        Fragment::new(branch_id, exit_id)
    }

    fn construct_seq(&mut self, seq: &SeqExpr, ctx: NavContext) -> Fragment {
        let items: Vec<_> = seq.items().collect();

        // Uncaptured sequences don't create object scope - they just group items.
        // Captures propagate to parent scope. Object scope is created by:
        // - Captured sequences ({...} @name) via construct_capture
        // - QIS quantifiers that wrap loop body with StartObject/EndObject

        let start_id = self.graph.add_epsilon();
        self.graph.node_mut(start_id).set_nav(ctx.to_nav());

        let (child_fragments, _exit_ctx) = self.construct_item_sequence(&items, false);
        let inner = self.graph.sequence(&child_fragments);

        self.graph.connect(start_id, inner.entry);

        Fragment::new(start_id, inner.exit)
    }

    fn construct_capture(&mut self, cap: &CapturedExpr, ctx: NavContext) -> Fragment {
        let Some(inner_expr) = cap.inner() else {
            return self.graph.epsilon_fragment();
        };

        let inner_frag = self.construct_expr(&inner_expr, ctx);

        let capture_token = cap.name();
        let capture_name = capture_token.as_ref().map(|t| token_src(t, self.source));

        let has_to_string = cap
            .type_annotation()
            .and_then(|t| t.name())
            .map(|n| n.text() == "string")
            .unwrap_or(false);

        // Captured sequence/alternation creates object scope for nested fields.
        // Tagged alternations use variants instead (handled in construct_tagged_alt).
        // Quantifiers only need wrapper if QIS (2+ captures) - otherwise the array is the direct value.
        let needs_object_wrapper = match &inner_expr {
            Expr::SeqExpr(_) | Expr::AltExpr(_) => true,
            Expr::QuantifiedExpr(q) => self.qis_triggers.contains_key(q),
            _ => false,
        };

        let matchers = self.find_all_matchers(inner_frag.entry);
        for matcher_id in matchers {
            self.graph
                .node_mut(matcher_id)
                .add_effect(BuildEffect::CaptureNode);

            if has_to_string {
                self.graph
                    .node_mut(matcher_id)
                    .add_effect(BuildEffect::ToString);
            }
        }

        if let Some(name) = capture_name {
            let span = capture_token
                .as_ref()
                .map(|t| t.text_range())
                .unwrap_or_default();

            let (entry, exit) = if needs_object_wrapper {
                // Wrap with StartObject/EndObject for composite captures
                let start_id = self.graph.add_epsilon();
                self.graph
                    .node_mut(start_id)
                    .add_effect(BuildEffect::StartObject);
                self.graph.connect(start_id, inner_frag.entry);

                let end_id = self.graph.add_epsilon();
                self.graph
                    .node_mut(end_id)
                    .add_effect(BuildEffect::EndObject);
                self.graph.connect(inner_frag.exit, end_id);

                (start_id, end_id)
            } else {
                (inner_frag.entry, inner_frag.exit)
            };

            let field_id = self.graph.add_epsilon();
            self.graph
                .node_mut(field_id)
                .add_effect(BuildEffect::Field { name, span });
            self.graph.connect(exit, field_id);
            Fragment::new(entry, field_id)
        } else {
            inner_frag
        }
    }

    fn construct_quantifier(&mut self, quant: &QuantifiedExpr, ctx: NavContext) -> Fragment {
        let Some(inner_expr) = quant.inner() else {
            return self.graph.epsilon_fragment();
        };
        let Some(op) = quant.operator() else {
            return self.construct_expr(&inner_expr, ctx);
        };

        let inner_frag = self.construct_expr(&inner_expr, ctx);
        let is_qis = self.qis_triggers.contains_key(quant);

        match op.kind() {
            SyntaxKind::Star if is_qis => self.graph.zero_or_more_array_qis(inner_frag),
            SyntaxKind::Star => self.graph.zero_or_more_array(inner_frag),
            SyntaxKind::StarQuestion if is_qis => {
                self.graph.zero_or_more_array_qis_lazy(inner_frag)
            }
            SyntaxKind::StarQuestion => self.graph.zero_or_more_array_lazy(inner_frag),
            SyntaxKind::Plus if is_qis => self.graph.one_or_more_array_qis(inner_frag),
            SyntaxKind::Plus => self.graph.one_or_more_array(inner_frag),
            SyntaxKind::PlusQuestion if is_qis => self.graph.one_or_more_array_qis_lazy(inner_frag),
            SyntaxKind::PlusQuestion => self.graph.one_or_more_array_lazy(inner_frag),
            SyntaxKind::Question if is_qis => self.graph.optional_qis(inner_frag),
            SyntaxKind::Question => self.graph.optional(inner_frag),
            SyntaxKind::QuestionQuestion if is_qis => self.graph.optional_qis_lazy(inner_frag),
            SyntaxKind::QuestionQuestion => self.graph.optional_lazy(inner_frag),
            _ => inner_frag,
        }
    }

    fn construct_field(&mut self, field: &FieldExpr, ctx: NavContext) -> Fragment {
        let Some(value_expr) = field.value() else {
            return self.graph.epsilon_fragment();
        };
        self.construct_expr(&value_expr, ctx)
    }

    fn find_field_constraint(&self, node: &crate::parser::SyntaxNode) -> Option<&'a str> {
        let parent = node.parent()?;
        let field_expr = FieldExpr::cast(parent)?;
        let name_token = field_expr.name()?;
        Some(token_src(&name_token, self.source))
    }

    fn find_all_matchers(&self, start: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        self.collect_matchers(start, &mut result, &mut visited);
        result
    }

    fn collect_matchers(
        &self,
        node_id: NodeId,
        result: &mut Vec<NodeId>,
        visited: &mut HashSet<NodeId>,
    ) {
        if !visited.insert(node_id) {
            return;
        }

        let node = self.graph.node(node_id);
        if !node.is_epsilon() {
            result.push(node_id);
            return;
        }

        for &succ in &node.successors {
            self.collect_matchers(succ, result, visited);
        }
    }
}

fn is_anonymous_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::AnonymousNode(n) if !n.is_any())
}
