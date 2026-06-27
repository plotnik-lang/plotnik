use std::collections::{HashMap, HashSet};

use crate::core::{Cardinality, NodeFieldId, NodeKindId};

use super::structure::{SkeletonVariable, StepTarget, VarId};
use super::types::{Grammar, NodeConstraints};

/// Insertion-ordered set of node kinds: dedups while keeping first-seen order, so derived
/// reachability lists are deterministic. [`into_sorted`](Self::into_sorted) reorders by
/// public name for legible diagnostics.
#[derive(Default)]
pub(super) struct KindSet {
    kinds: Vec<NodeKindId>,
    seen: HashSet<NodeKindId>,
}

impl KindSet {
    pub(super) fn insert(&mut self, id: NodeKindId) {
        if self.seen.insert(id) {
            self.kinds.push(id);
        }
    }

    pub(super) fn into_vec(self) -> Vec<NodeKindId> {
        self.kinds
    }

    fn into_sorted(mut self, grammar: &Grammar) -> Vec<NodeKindId> {
        self.kinds
            .sort_unstable_by(|a, b| grammar.node_kind(*a).cmp(&grammar.node_kind(*b)));
        self.kinds
    }
}

#[derive(Debug, Clone)]
pub(super) struct AdmissibilityIndex {
    children: HashMap<NodeKindId, Vec<NodeKindId>>,
    fields: HashMap<(NodeKindId, NodeFieldId), AdmissibleField>,
    /// Per-node field-id index backing [`Grammar::field_ids_for_node_kind`],
    /// pre-grouped so the lookup is O(fields-on-node) rather than a scan of every
    /// `(node, field)` key.
    fields_by_node: HashMap<NodeKindId, Vec<NodeFieldId>>,
}

#[derive(Debug, Clone)]
struct AdmissibleField {
    valid_types: Vec<NodeKindId>,
    /// Summary-derived cardinality, or `None` when the field exists only because
    /// structural reachability recovered it.
    cardinality: Option<Cardinality>,
}

impl AdmissibilityIndex {
    pub(super) fn from_summary(
        node_constraints: &HashMap<NodeKindId, NodeConstraints>,
        field_names: &HashMap<NodeFieldId, String>,
    ) -> Self {
        let mut children = HashMap::new();
        let mut fields = HashMap::new();
        let mut fields_by_node = HashMap::new();

        for (&node, constraints) in node_constraints {
            if let Some(child_constraints) = &constraints.children {
                children.insert(node, child_constraints.valid_types.clone());
            }

            let mut node_fields = Vec::new();
            for (&field, field_constraints) in &constraints.fields {
                fields.insert(
                    (node, field),
                    AdmissibleField {
                        valid_types: field_constraints.valid_types.clone(),
                        cardinality: Some(field_constraints.cardinality),
                    },
                );
                node_fields.push(field);
            }
            if !node_fields.is_empty() {
                node_fields.sort_unstable_by(|a, b| {
                    let a = field_names
                        .get(a)
                        .expect("admissible field id must have a name");
                    let b = field_names
                        .get(b)
                        .expect("admissible field id must have a name");
                    a.cmp(b)
                });
                fields_by_node.insert(node, node_fields);
            }
        }

        Self {
            children,
            fields,
            fields_by_node,
        }
    }

    pub(super) fn from_reachability(reachability: Reachability) -> Self {
        Self {
            children: reachability.children,
            fields: reachability.fields,
            fields_by_node: reachability.fields_by_node,
        }
    }

    pub(super) fn field_ids_for_node_kind(&self, node_kind_id: NodeKindId) -> &[NodeFieldId] {
        self.fields_by_node
            .get(&node_kind_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub(super) fn has_field(&self, node_kind_id: NodeKindId, node_field_id: NodeFieldId) -> bool {
        self.fields.contains_key(&(node_kind_id, node_field_id))
    }

    pub(super) fn valid_field_types(
        &self,
        node_kind_id: NodeKindId,
        node_field_id: NodeFieldId,
    ) -> &[NodeKindId] {
        self.fields
            .get(&(node_kind_id, node_field_id))
            .map(|field| field.valid_types.as_slice())
            .unwrap_or(&[])
    }

    pub(super) fn field_cardinality(
        &self,
        node_kind_id: NodeKindId,
        node_field_id: NodeFieldId,
    ) -> Option<Cardinality> {
        self.fields
            .get(&(node_kind_id, node_field_id))
            .and_then(|field| field.cardinality)
    }

    pub(super) fn valid_child_types(&self, node_kind_id: NodeKindId) -> &[NodeKindId] {
        self.children
            .get(&node_kind_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

/// Child- and field-kind reachability for every node kind — the model the structural
/// grammar check queries for local admissibility. It is derived from the same ordered
/// skeleton that satisfiability threads, so the two checks stay conservatively aligned
/// without sharing one runtime model.
///
/// It is the **union of two derivations of the same grammar, each correct but lossy in a
/// different place**, so either one alone would reject queries the grammar actually allows:
///
/// - The **node-shape summary** applies tree-sitter's full field/child resolution — including
///   bubbling a sibling field out of a hidden field-value rule (gleam `let.assign`, carried by
///   the hidden `_pattern` that is `let`'s `pattern` value) — but flattens supertypes away.
/// - The **structural skeleton** *points* (a step records a public id and/or a descent body);
///   descending it recovers what the summary drops by expanding **transparent** positions,
///   whose contents tree-sitter surfaces on the enclosing concrete node:
///   - an *id-less inlined rule* is fully transparent: its children and fields both surface on
///     the parent (python `(module (match_statement))`, via the hidden `_statement` chain);
///   - a *supertype* is inlined at runtime, so a field on one of its members surfaces on the
///     parent (lua `chunk.local_declaration`). Its children are already covered by recording
///     the supertype id and expanding it via `collect_subtypes`, so it is descended for member
///     fields alone;
///   - an *inlined-supertype field value* surfaces concrete kinds the summary never resolved
///     (go `var_spec.type: (slice_type)`, typed by the inlined `_type`).
///
/// Their union over-approximates possibility — sound, since over-listing only widens
/// admissibility (a tolerated false accept), never rejects a valid query.
pub(super) struct Reachability {
    children: HashMap<NodeKindId, Vec<NodeKindId>>,
    fields: HashMap<(NodeKindId, NodeFieldId), AdmissibleField>,
    /// The `fields` keys grouped by node into a name-sorted per-node index, so
    /// [`Grammar::field_ids_for_node_kind`] is a single map lookup rather than a scan of every key.
    fields_by_node: HashMap<NodeKindId, Vec<NodeFieldId>>,
}

impl Reachability {
    pub(super) fn compute(
        grammar: &Grammar,
        node_constraints: &HashMap<NodeKindId, NodeConstraints>,
    ) -> Self {
        let mut builder = ReachabilityBuilder {
            grammar,
            node_constraints,
            children: HashMap::new(),
            fields: HashMap::new(),
        };
        builder.seed_from_summary();
        for (kind, realizers) in variable_realizers_by_kind(grammar) {
            for realizer in realizers {
                builder.collect_node(kind, realizer, &mut HashSet::new());
            }
        }
        builder.finish()
    }
}

struct ReachabilityBuilder<'g> {
    grammar: &'g Grammar,
    node_constraints: &'g HashMap<NodeKindId, NodeConstraints>,
    children: HashMap<NodeKindId, KindSet>,
    fields: HashMap<(NodeKindId, NodeFieldId), FieldReachability>,
}

#[derive(Default)]
struct FieldReachability {
    types: KindSet,
    cardinality: Option<Cardinality>,
}

impl ReachabilityBuilder<'_> {
    /// Baseline of the union: every child and field the node-shape summary already resolved,
    /// so the structural pass can only widen the result, never list fewer than the summary.
    fn seed_from_summary(&mut self) {
        for (&node, constraints) in self.node_constraints {
            if let Some(children) = &constraints.children {
                let set = self.children.entry(node).or_default();
                for &id in &children.valid_types {
                    set.insert(id);
                }
            }
            for (&field, field_constraints) in &constraints.fields {
                let reachable = self.fields.entry((node, field)).or_default();
                reachable.cardinality = Some(field_constraints.cardinality);
                for &id in &field_constraints.valid_types {
                    reachable.types.insert(id);
                }
            }
        }
    }

    fn finish(self) -> Reachability {
        let grammar = self.grammar;
        let children = self
            .children
            .into_iter()
            .map(|(kind, set)| (kind, set.into_sorted(grammar)))
            .collect();
        let fields: HashMap<(NodeKindId, NodeFieldId), AdmissibleField> = self
            .fields
            .into_iter()
            .map(|(key, field)| {
                (
                    key,
                    AdmissibleField {
                        valid_types: field.types.into_sorted(grammar),
                        cardinality: field.cardinality,
                    },
                )
            })
            .collect();
        let mut fields_by_node: HashMap<NodeKindId, Vec<NodeFieldId>> = HashMap::new();
        for &(node, field) in fields.keys() {
            fields_by_node.entry(node).or_default().push(field);
        }
        for ids in fields_by_node.values_mut() {
            ids.sort_unstable_by(|a, b| {
                let a = grammar
                    .field_name(*a)
                    .expect("admissible field id must have a name");
                let b = grammar
                    .field_name(*b)
                    .expect("admissible field id must have a name");
                a.cmp(b)
            });
        }
        Reachability {
            children,
            fields,
            fields_by_node,
        }
    }

    /// Record the children and fields that surface directly on `node`, descending fully
    /// transparent id-less inlined rules (whose children and fields both surface here).
    fn collect_node(&mut self, node: NodeKindId, var: VarId, seen: &mut HashSet<VarId>) {
        if !seen.insert(var) {
            return;
        }
        let variable = structure_variable(self.grammar, var);
        for step in variable.productions.iter().flatten() {
            if let Some(field) = step.field {
                self.collect_field_value(node, field, step.target);
                continue;
            }
            if let Some(id) = step.target.id {
                if !self.grammar.is_anonymous_node(id) {
                    self.children.entry(node).or_default().insert(id);
                }
                if let Some(body) = step.target.transparent_body(self.grammar) {
                    self.collect_transparent_fields(node, body, &mut HashSet::new());
                }
            } else if let Some(body) = step.target.transparent_body(self.grammar) {
                self.collect_node(node, body, seen);
            }
        }
    }

    /// Collect, into `node`'s field set, fields that surface on it through a transparent
    /// subtree — a supertype or id-less inlined rule entered without crossing into a
    /// concrete child. A concrete child is opaque: its own fields stay with it, so it is not
    /// descended.
    fn collect_transparent_fields(
        &mut self,
        node: NodeKindId,
        var: VarId,
        seen: &mut HashSet<VarId>,
    ) {
        if !seen.insert(var) {
            return;
        }
        let variable = structure_variable(self.grammar, var);
        for step in variable.productions.iter().flatten() {
            if let Some(field) = step.field {
                self.collect_field_value(node, field, step.target);
                continue;
            }
            if let Some(body) = step.target.transparent_body(self.grammar) {
                self.collect_transparent_fields(node, body, seen);
            }
        }
    }

    /// Record the kinds a fielded step's value can take. A value under a public id —
    /// concrete kind, alias, or kept supertype — is recorded whole; a kept supertype is
    /// expanded downstream by `collect_subtypes`. An id-less inlined value (go's `_type`) is
    /// transparent: descend it to the concrete kinds it stands for. Anonymous kinds are kept
    /// — a field value may be a literal token (`operator: "+"`).
    fn collect_field_value(&mut self, node: NodeKindId, field: NodeFieldId, target: StepTarget) {
        let value = &mut self.fields.entry((node, field)).or_default().types;
        if let Some(id) = target.id {
            value.insert(id);
        } else if let Some(body) = target.idless_value_body() {
            collect_value_frontier(self.grammar, body, value, &mut HashSet::new());
        }
    }
}

/// Index each public node kind to the skeleton variables that realize it: the
/// variable named for it, plus every step occurrence (aliases included) that
/// surfaces it and descends into a variable body. Built from
/// [`StructureTable::surface_realizers_by_kind`], the same index the satisfiability
/// engine indexes, so both passes reason over the same model.
fn variable_realizers_by_kind(grammar: &Grammar) -> HashMap<NodeKindId, Vec<VarId>> {
    let mut realizers_by_kind: HashMap<NodeKindId, Vec<VarId>> = HashMap::new();
    for (kind, realizers) in grammar.structure().surface_realizers_by_kind() {
        for realizer in realizers {
            if let Some(body) = realizer.body {
                realizers_by_kind.entry(kind).or_default().push(body);
            }
        }
    }
    realizers_by_kind
}

/// Descend an id-less inlined field value to the concrete kinds it can surface as. Keeps
/// anonymous kinds (a field value may be a token) and ignores labels — a label inside an
/// inlined value still names a candidate value kind, and over-listing only widens
/// admissibility, never narrows it. A kept supertype within is recorded by id (expanded
/// downstream).
fn collect_value_frontier(
    grammar: &Grammar,
    var: VarId,
    out: &mut KindSet,
    seen: &mut HashSet<VarId>,
) {
    if !seen.insert(var) {
        return;
    }
    let variable = structure_variable(grammar, var);
    for step in variable.productions.iter().flatten() {
        if let Some(id) = step.target.id {
            out.insert(id);
        } else if let Some(body) = step.target.idless_value_body() {
            collect_value_frontier(grammar, body, out, seen);
        }
    }
}

fn structure_variable(grammar: &Grammar, var: VarId) -> &SkeletonVariable {
    grammar
        .structure()
        .variable(var)
        .expect("VarId from StructureTable must resolve")
}
