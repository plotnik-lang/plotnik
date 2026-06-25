use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::core::{Cardinality, NodeFieldId, NodeKind, NodeKindId};

use super::json::GrammarError;
use super::raw::RawGrammar;
use super::render::TreeGrammar;
use super::structure::StructureTable;

pub(super) struct GrammarTables {
    pub(super) node_shapes: Vec<NodeShape>,
    pub(super) symbols: Vec<NodeKindEntry>,
    pub(super) fields: Vec<FieldEntry>,
}

#[derive(Debug, Clone)]
pub(super) struct NodeKindEntry {
    pub(super) id: u16,
    pub(super) kind_name: String,
    pub(super) named: bool,
    pub(super) visible: bool,
    pub(super) supertype: bool,
    /// True when this symbol is a lexical terminal or an external token. A public
    /// node kind is a leaf token only when *every* contributing symbol is terminal
    /// (see `token_node_ids`), which distinguishes real leaves like `identifier`
    /// from childless syntax nodes like `debugger_statement`.
    pub(super) terminal: bool,
}

impl NodeKindEntry {
    pub(super) fn alias(id: u16, kind_name: String, named: bool) -> Self {
        Self {
            id,
            kind_name,
            named,
            visible: true,
            supertype: false,
            // Aliases get fresh ids, so this value never reaches the aliased public
            // id's per-kind accumulation. Token detection also checks node shape, so
            // `false` is the safe default for alias-only public ids.
            terminal: false,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct FieldEntry {
    pub(super) id: u16,
    pub(super) name: String,
}

/// Tree-sitter grammar plus Plotnik's derived compile-time metadata.
#[derive(Debug, Clone)]
pub struct Grammar {
    name: String,
    node_constraints: HashMap<NodeKindId, NodeConstraints>,
    extra_node_kinds: Vec<NodeKindId>,
    root_node_kind: Option<NodeKindId>,
    named_node_ids: HashMap<String, NodeKindId>,
    anonymous_node_ids: HashMap<String, NodeKindId>,
    node_names: HashMap<NodeKindId, String>,
    field_ids: HashMap<String, NodeFieldId>,
    field_names: HashMap<NodeFieldId, String>,
    supertype_ids: HashSet<NodeKindId>,
    subtypes: HashMap<NodeKindId, Vec<NodeKindId>>,
    /// Public node kinds that are leaf tokens (no child nodes are derivable).
    token_node_ids: HashSet<NodeKindId>,
    /// Public node ids that name anonymous (literal-token) kinds.
    anonymous_node_id_set: HashSet<NodeKindId>,
    fields_by_node: HashMap<NodeKindId, Vec<String>>,
    all_named_node_kinds: Vec<String>,
    all_anonymous_node_kinds: Vec<String>,
    all_field_names: Vec<String>,
    structure: StructureTable,
    /// Tree-shape rendering model for `lang dump` and diagnostics. Populated only
    /// on the `from_raw` path (the pipeline data it taps does not survive into
    /// `GrammarTables`); empty otherwise.
    tree: TreeGrammar,
}

impl Grammar {
    /// Build production grammar metadata from a raw source-format grammar.
    pub fn from_raw(raw: &RawGrammar) -> Result<Self, GrammarError> {
        use super::{
            aliases::extract_default_aliases,
            lower::{LoweredGrammar, derive_fields, derive_symbols},
            node_shapes,
            productions::{expand_repeats, flatten_grammar, process_inlines},
            symbols::intern_symbols,
            tokens::{expand_tokens, extract_tokens},
            validation::{validate_indirect_recursion, validate_precedences},
        };

        let mut lowered = LoweredGrammar::from_raw(raw);

        lowered.retain_reachable_rules();
        validate_precedences(&lowered.variables, &lowered.precedence_orderings)
            .map_err(|error| GrammarError::Analysis(error.to_string()))?;
        validate_indirect_recursion(&lowered.variables)
            .map_err(|error| GrammarError::Analysis(error.to_string()))?;

        let resolved_grammar = intern_symbols(lowered.as_uninterned())
            .map_err(|error| GrammarError::Analysis(error.to_string()))?;
        let (syntax_grammar, lexical_grammar) = extract_tokens(resolved_grammar)
            .map_err(|error| GrammarError::Analysis(error.to_string()))?;
        // Tap the pipeline here: nested Seq/Choice/Repeat structure is intact and
        // tokens are split out, but `expand_repeats`/`flatten` (which consume these
        // grammars below) would render them unusable as documentation. Lower the
        // shapes now, in place, so the grammars never have to be cloned; the
        // closure-dependent bits are attached from `node_shapes` further down.
        let rule_order: Vec<String> = raw.rules.keys().cloned().collect();
        let (mut tree, tree_aliases) =
            super::render::lower(raw.name.clone(), &rule_order, &syntax_grammar, &lexical_grammar);
        let syntax_grammar = expand_repeats(syntax_grammar);
        let mut syntax_grammar = flatten_grammar(syntax_grammar)
            .map_err(|error| GrammarError::Analysis(error.to_string()))?;
        let lexical_grammar = expand_tokens(lexical_grammar)
            .map_err(|error| GrammarError::Analysis(error.to_string()))?;
        let aliases = extract_default_aliases(&mut syntax_grammar, &lexical_grammar);
        let inlines = process_inlines(&syntax_grammar, &lexical_grammar)
            .map_err(|error| GrammarError::Analysis(error.to_string()))?;

        let grammar_ctx = node_shapes::GrammarContext {
            syntax: &syntax_grammar,
            lexical: &lexical_grammar,
            aliases: &aliases,
        };
        let variable_summaries = node_shapes::derive_variable_summaries(grammar_ctx)
            .map_err(|error| GrammarError::Analysis(error.to_string()))?;
        let node_shapes = node_shapes::generate_node_shapes(grammar_ctx, &variable_summaries)
            .map_err(|error| GrammarError::Analysis(error.to_string()))?;

        super::render::attach_node_shapes(&mut tree, &node_shapes, &tree_aliases);

        let tables = GrammarTables {
            node_shapes,
            symbols: derive_symbols(&syntax_grammar, &lexical_grammar, &inlines, &aliases),
            fields: derive_fields(&syntax_grammar, &inlines, &variable_summaries),
        };

        let mut grammar =
            Self::from_tables(raw.name.clone(), tables).map_err(GrammarError::Analysis)?;
        let structure = StructureTable::build(grammar_ctx, &grammar);
        grammar.structure = structure;
        grammar.tree = tree;
        Ok(grammar)
    }

    pub(super) fn from_tables(name: String, tables: GrammarTables) -> Result<Self, String> {
        let mut named_node_ids = HashMap::new();
        let mut anonymous_node_ids = HashMap::new();
        let mut node_names = HashMap::new();
        let mut supertype_ids = HashSet::new();

        // A public node kind can be reached by multiple symbols (e.g. via aliases). It is a leaf
        // token only when every contributing symbol is terminal, so this accumulates per kind with
        // AND: a single non-terminal contributor (like `debugger_statement` = `"debugger" ";"`)
        // keeps the kind out of the token set even when it otherwise looks childless.
        let mut all_terminal: HashMap<NodeKindId, bool> = HashMap::new();

        for symbol in tables.symbols {
            let node_id = node_kind_id(symbol.id);
            node_names.insert(node_id, symbol.kind_name.clone());

            all_terminal
                .entry(node_id)
                .and_modify(|every| *every &= symbol.terminal)
                .or_insert(symbol.terminal);

            if symbol.supertype {
                supertype_ids.insert(node_id);
            }

            if !symbol.visible && !symbol.supertype {
                continue;
            }

            if symbol.named {
                named_node_ids.entry(symbol.kind_name).or_insert(node_id);
            } else {
                anonymous_node_ids
                    .entry(symbol.kind_name)
                    .or_insert(node_id);
            }
        }

        let mut field_ids = HashMap::new();
        let mut field_names = HashMap::new();
        for field in tables.fields {
            let field_id = node_field_id(field.id);
            field_names.insert(field_id, field.name.clone());
            field_ids.insert(field.name, field_id);
        }

        let (node_constraints, extra_node_kinds, root_node_kind) = build_node_constraints(
            &tables.node_shapes,
            |node_kind| resolve_node_id(&named_node_ids, &anonymous_node_ids, node_kind),
            |name| field_ids.get(name).copied(),
        )
        .map_err(|error| error.to_string())?;

        let mut subtypes = HashMap::new();
        for shape in &tables.node_shapes {
            let Some(shape_subtypes) = &shape.subtypes else {
                continue;
            };
            let Some(supertype) =
                resolve_node_id(&named_node_ids, &anonymous_node_ids, shape.node_kind())
            else {
                continue;
            };

            let resolved = shape_subtypes
                .iter()
                .filter_map(|subtype| {
                    resolve_node_id(&named_node_ids, &anonymous_node_ids, subtype.node_kind())
                })
                .collect::<Vec<_>>();
            subtypes.insert(supertype, resolved);
        }

        let mut fields_by_node = HashMap::new();
        for shape in &tables.node_shapes {
            let Some(node_id) =
                resolve_node_id(&named_node_ids, &anonymous_node_ids, shape.node_kind())
            else {
                continue;
            };
            let mut fields = shape.fields.keys().cloned().collect::<Vec<_>>();
            fields.sort();
            fields_by_node.insert(node_id, fields);
        }

        let mut all_named_node_kinds = named_node_ids.keys().cloned().collect::<Vec<_>>();
        all_named_node_kinds.sort();

        let mut all_anonymous_node_kinds = anonymous_node_ids.keys().cloned().collect::<Vec<_>>();
        all_anonymous_node_kinds.sort();

        let mut all_field_names = field_ids.keys().cloned().collect::<Vec<_>>();
        all_field_names.sort();

        // A kind is a leaf token only when it is terminal AND its shape declares no children and
        // no fields. The terminal flag alone is not enough: alias-introduced symbols receive fresh
        // node ids (see `derive_symbols`), so their non-terminal `terminal: false` never reaches
        // the public id's per-kind accumulation. A kind reachable both by a terminal symbol and by
        // children-bearing aliases then accumulates to all-terminal even though it has a real
        // children slot. Confirming against the node shape handles this: a kind with a
        // children/fields slot is never a leaf token, so a named child under it is valid.
        let token_node_ids = all_terminal
            .into_iter()
            .filter(|&(_, every_terminal)| every_terminal)
            .filter_map(|(node_id, _)| {
                let constraints = node_constraints.get(&node_id);
                let has_children = constraints
                    .and_then(|c| c.children.as_ref())
                    .is_some_and(|children| !children.valid_types.is_empty());
                let has_fields = constraints.is_some_and(|c| !c.fields.is_empty());
                (!has_children && !has_fields).then_some(node_id)
            })
            .collect::<HashSet<_>>();

        let anonymous_node_id_set = anonymous_node_ids.values().copied().collect::<HashSet<_>>();

        Ok(Self {
            name,
            node_constraints,
            extra_node_kinds,
            root_node_kind,
            named_node_ids,
            anonymous_node_ids,
            node_names,
            field_ids,
            field_names,
            supertype_ids,
            subtypes,
            token_node_ids,
            anonymous_node_id_set,
            fields_by_node,
            all_named_node_kinds,
            all_anonymous_node_kinds,
            all_field_names,
            // Populated only on the `from_raw` path: `GrammarTables` has already
            // discarded the flattened productions the table distills.
            structure: StructureTable::default(),
            tree: TreeGrammar::default(),
        })
    }

    /// Grammar name (e.g., "javascript", "rust").
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Distilled structural skeleton of the grammar's productions — ordered,
    /// visibility-classified step sequences retained from the flattened grammar
    /// before it is discarded. Empty for grammars built directly from metadata:
    /// the flattened productions it distills do not survive into `GrammarTables`,
    /// so only the `from_raw` path can populate it.
    pub fn structure(&self) -> &StructureTable {
        &self.structure
    }

    /// Tree-shape rendering model for `lang dump` and diagnostics. Empty for
    /// grammars built directly from metadata rather than through `from_raw`.
    pub fn tree(&self) -> &TreeGrammar {
        &self.tree
    }

    pub fn resolve_named_node(&self, kind: &str) -> Option<NodeKindId> {
        self.named_node_ids.get(kind).copied()
    }

    pub fn resolve_anonymous_node(&self, kind: &str) -> Option<NodeKindId> {
        self.anonymous_node_ids.get(kind).copied()
    }

    pub fn resolve_field(&self, field: &str) -> Option<NodeFieldId> {
        self.field_ids.get(field).copied()
    }

    /// Human-readable node kind for diagnostics/debugging.
    pub fn node_kind(&self, node_kind_id: NodeKindId) -> Option<&str> {
        self.node_names.get(&node_kind_id).map(String::as_str)
    }

    /// Human-readable field name for diagnostics/debugging.
    pub fn field_name(&self, node_field_id: NodeFieldId) -> Option<&str> {
        self.field_names.get(&node_field_id).map(String::as_str)
    }

    pub fn all_named_node_kinds(&self) -> Vec<&str> {
        self.all_named_node_kinds
            .iter()
            .map(String::as_str)
            .collect()
    }

    pub fn all_anonymous_node_kinds(&self) -> Vec<&str> {
        self.all_anonymous_node_kinds
            .iter()
            .map(String::as_str)
            .collect()
    }

    pub fn all_field_names(&self) -> Vec<&str> {
        self.all_field_names.iter().map(String::as_str).collect()
    }

    pub fn fields_for_node_kind(&self, node_kind_id: NodeKindId) -> Vec<&str> {
        self.fields_by_node
            .get(&node_kind_id)
            .map(|fields| fields.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    pub fn is_supertype(&self, node_kind_id: NodeKindId) -> bool {
        self.supertype_ids.contains(&node_kind_id)
    }

    pub fn subtypes(&self, supertype: NodeKindId) -> &[NodeKindId] {
        self.subtypes
            .get(&supertype)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Transitive closure of a supertype's subtypes (direct and indirect), excluding the
    /// supertype itself. `subtypes` is direct-only, so structural validation expands it here.
    /// Sequence validation calls it too; centralized here to avoid duplicating the traversal.
    pub fn collect_subtypes(&self, supertype: NodeKindId) -> HashSet<NodeKindId> {
        let mut closure = HashSet::new();
        let mut stack = vec![supertype];
        while let Some(node) = stack.pop() {
            for &sub in self.subtypes(node) {
                if closure.insert(sub) {
                    stack.push(sub);
                }
            }
        }
        closure
    }

    /// Whether `node_kind_id` is a leaf token kind — a node whose content is its own text and
    /// which never has child nodes (e.g. `identifier`). Childless *syntax* nodes such as
    /// `debugger_statement` are not tokens: extras like comments can still attach beneath them.
    pub fn is_token(&self, node_kind_id: NodeKindId) -> bool {
        self.token_node_ids.contains(&node_kind_id)
    }

    /// Whether `node_kind_id` names an anonymous (literal-token) kind, e.g. `"+"`. Used to
    /// distinguish fields that only accept literal tokens (where a `(_)` value is impossible).
    pub fn is_anonymous_node(&self, node_kind_id: NodeKindId) -> bool {
        self.anonymous_node_id_set.contains(&node_kind_id)
    }

    pub fn root(&self) -> Option<NodeKindId> {
        self.root_node_kind
    }

    pub fn is_extra(&self, node_kind_id: NodeKindId) -> bool {
        self.extra_node_kinds.contains(&node_kind_id)
    }

    pub fn has_field(&self, node_kind_id: NodeKindId, node_field_id: NodeFieldId) -> bool {
        self.node_constraints
            .get(&node_kind_id)
            .is_some_and(|constraints| constraints.fields.contains_key(&node_field_id))
    }

    pub fn field_cardinality(
        &self,
        node_kind_id: NodeKindId,
        node_field_id: NodeFieldId,
    ) -> Option<Cardinality> {
        self.field_constraints(node_kind_id, node_field_id)
            .map(|field| field.cardinality)
    }

    pub fn valid_field_types(
        &self,
        node_kind_id: NodeKindId,
        node_field_id: NodeFieldId,
    ) -> &[NodeKindId] {
        self.field_constraints(node_kind_id, node_field_id)
            .map(|field| field.valid_types.as_slice())
            .unwrap_or(&[])
    }

    pub fn is_valid_field_type(
        &self,
        node_kind_id: NodeKindId,
        node_field_id: NodeFieldId,
        child: NodeKindId,
    ) -> bool {
        self.valid_field_types(node_kind_id, node_field_id)
            .contains(&child)
    }

    pub fn children_cardinality(&self, node_kind_id: NodeKindId) -> Option<Cardinality> {
        self.children_constraints(node_kind_id)
            .map(|children| children.cardinality)
    }

    pub fn valid_child_types(&self, node_kind_id: NodeKindId) -> &[NodeKindId] {
        self.children_constraints(node_kind_id)
            .map(|children| children.valid_types.as_slice())
            .unwrap_or(&[])
    }

    pub fn is_valid_child_type(&self, node_kind_id: NodeKindId, child: NodeKindId) -> bool {
        self.valid_child_types(node_kind_id).contains(&child)
    }

    fn field_constraints(
        &self,
        node_kind_id: NodeKindId,
        field_id: NodeFieldId,
    ) -> Option<&FieldConstraints> {
        self.node_constraints_for(node_kind_id)
            .fields
            .get(&field_id)
    }

    fn children_constraints(&self, node_kind_id: NodeKindId) -> Option<&ChildrenConstraints> {
        self.node_constraints_for(node_kind_id).children.as_ref()
    }

    fn node_constraints_for(&self, node_kind_id: NodeKindId) -> &NodeConstraints {
        // Leaf-token kinds (and some alias-produced ids) carry no node shape, so they have no
        // constraints entry. Treat them as having no children/fields rather than panicking: a
        // token has no children, so an empty view is correct, and any named child under it is
        // already handled by the `is_token` leaf check. Returning empty keeps every constraint lookup
        // (admissibility, predicate-on-leaf, hints) total instead of crashing on such ids.
        static EMPTY: LazyLock<NodeConstraints> = LazyLock::new(|| NodeConstraints {
            fields: HashMap::new(),
            children: None,
        });
        self.node_constraints.get(&node_kind_id).unwrap_or(&EMPTY)
    }
}

fn node_kind_id(id: u16) -> NodeKindId {
    NodeKindId::try_from(id).expect("lowered node symbol id must be non-zero in production grammar")
}

/// Resolve a node kind to its id via the public name maps. Equivalent to the former
/// `node_ids: HashMap<NodeKind<&str>, _>`: both maps are populated in lockstep with it, so the
/// lookups match — but these own their keys, freeing the symbol loop to consume `tables.symbols`.
fn resolve_node_id(
    named_node_ids: &HashMap<String, NodeKindId>,
    anonymous_node_ids: &HashMap<String, NodeKindId>,
    node_kind: NodeKind<&str>,
) -> Option<NodeKindId> {
    match node_kind {
        NodeKind::Named(name) => named_node_ids.get(name).copied(),
        NodeKind::Anonymous(name) => anonymous_node_ids.get(name).copied(),
    }
}

fn node_field_id(id: u16) -> NodeFieldId {
    NodeFieldId::try_from(id)
        .expect("lowered field symbol id must be non-zero in production grammar")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct NodeShape {
    #[serde(rename = "type")]
    pub(crate) kind_name: String,
    pub(crate) named: bool,
    #[serde(default)]
    pub(crate) root: bool,
    #[serde(default)]
    pub(crate) extra: bool,
    #[serde(default)]
    pub(crate) fields: HashMap<String, NodeSlot>,
    pub(crate) children: Option<NodeSlot>,
    pub(crate) subtypes: Option<Vec<NodeKindRef>>,
}

impl NodeShape {
    fn node_kind(&self) -> NodeKind<&str> {
        if self.named {
            NodeKind::Named(self.kind_name.as_str())
        } else {
            NodeKind::Anonymous(self.kind_name.as_str())
        }
    }
}

/// Cardinality constraints for a field or children slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct NodeSlot {
    pub(crate) multiple: bool,
    pub(crate) required: bool,
    pub(crate) types: Vec<NodeKindRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct NodeKindRef {
    #[serde(rename = "type")]
    pub(crate) kind_name: String,
    pub(crate) named: bool,
}

impl NodeKindRef {
    fn node_kind(&self) -> NodeKind<&str> {
        if self.named {
            NodeKind::Named(self.kind_name.as_str())
        } else {
            NodeKind::Anonymous(self.kind_name.as_str())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NodeShapeBuildError {
    Field {
        node_kind: String,
        field: String,
    },
    FieldType {
        node_kind: String,
        field: String,
        kind: String,
        named: bool,
    },
    ChildType {
        node_kind: String,
        kind: String,
        named: bool,
    },
}

impl std::fmt::Display for NodeShapeBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Field { node_kind, field } => {
                write!(f, "unknown field {field:?} on node kind {node_kind:?}")
            }
            Self::FieldType {
                node_kind,
                field,
                kind,
                named,
            } => write!(
                f,
                "unknown field type {kind:?} (named: {named}) for field {field:?} on node kind {node_kind:?}"
            ),
            Self::ChildType {
                node_kind,
                kind,
                named,
            } => write!(
                f,
                "unknown child type {kind:?} (named: {named}) for node kind {node_kind:?}"
            ),
        }
    }
}

impl std::error::Error for NodeShapeBuildError {}

#[derive(Debug, Clone)]
pub(crate) struct FieldConstraints {
    pub(crate) cardinality: Cardinality,
    pub(crate) valid_types: Vec<NodeKindId>,
}

#[derive(Debug, Clone)]
pub(crate) struct ChildrenConstraints {
    pub(crate) cardinality: Cardinality,
    pub(crate) valid_types: Vec<NodeKindId>,
}

#[derive(Debug, Clone)]
pub(crate) struct NodeConstraints {
    pub(crate) fields: HashMap<NodeFieldId, FieldConstraints>,
    pub(crate) children: Option<ChildrenConstraints>,
}

type NodeConstraintBuild = (
    HashMap<NodeKindId, NodeConstraints>,
    Vec<NodeKindId>,
    Option<NodeKindId>,
);

pub(crate) fn build_node_constraints<F, G>(
    node_shapes: &[NodeShape],
    node_id_for_type: F,
    field_id_for_name: G,
) -> Result<NodeConstraintBuild, NodeShapeBuildError>
where
    F: Fn(NodeKind<&str>) -> Option<NodeKindId>,
    G: Fn(&str) -> Option<NodeFieldId>,
{
    let mut node_constraints = HashMap::new();
    let mut extra_node_kinds = Vec::new();
    let mut root_node_kind = None;
    let known_shapes = node_shapes
        .iter()
        .map(NodeShape::node_kind)
        .collect::<HashSet<_>>();

    for shape in node_shapes {
        let Some(node_id) = node_id_for_type(shape.node_kind()) else {
            continue;
        };

        if shape.root {
            root_node_kind = Some(node_id);
        }

        if shape.extra {
            extra_node_kinds.push(node_id);
        }

        let fields =
            build_field_constraints(shape, &known_shapes, &node_id_for_type, &field_id_for_name)?;
        let children = build_children_constraints(shape, &known_shapes, &node_id_for_type)?;

        node_constraints.insert(node_id, NodeConstraints { fields, children });
    }

    Ok((node_constraints, extra_node_kinds, root_node_kind))
}

fn build_field_constraints<F, G>(
    shape: &NodeShape,
    known_shapes: &HashSet<NodeKind<&str>>,
    node_id_for_type: &F,
    field_id_for_name: &G,
) -> Result<HashMap<NodeFieldId, FieldConstraints>, NodeShapeBuildError>
where
    F: Fn(NodeKind<&str>) -> Option<NodeKindId>,
    G: Fn(&str) -> Option<NodeFieldId>,
{
    let mut fields = HashMap::new();
    for (field_name, slot) in &shape.fields {
        let field_id = field_id_for_name(field_name).ok_or_else(|| NodeShapeBuildError::Field {
            node_kind: shape.kind_name.clone(),
            field: field_name.clone(),
        })?;

        let valid_types = resolve_slot_types(slot, known_shapes, node_id_for_type, |kind_ref| {
            NodeShapeBuildError::FieldType {
                node_kind: shape.kind_name.clone(),
                field: field_name.clone(),
                kind: kind_ref.kind_name.clone(),
                named: kind_ref.named,
            }
        })?;

        fields.insert(
            field_id,
            FieldConstraints {
                cardinality: Cardinality::from_flags(slot.multiple, slot.required),
                valid_types,
            },
        );
    }

    Ok(fields)
}

fn build_children_constraints<F>(
    shape: &NodeShape,
    known_shapes: &HashSet<NodeKind<&str>>,
    node_id_for_type: &F,
) -> Result<Option<ChildrenConstraints>, NodeShapeBuildError>
where
    F: Fn(NodeKind<&str>) -> Option<NodeKindId>,
{
    shape
        .children
        .as_ref()
        .map(|slot| {
            let valid_types =
                resolve_slot_types(slot, known_shapes, node_id_for_type, |kind_ref| {
                    NodeShapeBuildError::ChildType {
                        node_kind: shape.kind_name.clone(),
                        kind: kind_ref.kind_name.clone(),
                        named: kind_ref.named,
                    }
                })?;

            Ok(ChildrenConstraints {
                cardinality: Cardinality::from_flags(slot.multiple, slot.required),
                valid_types,
            })
        })
        .transpose()
}

fn resolve_slot_types<F, E>(
    slot: &NodeSlot,
    known_shapes: &HashSet<NodeKind<&str>>,
    node_id_for_type: &F,
    error: E,
) -> Result<Vec<NodeKindId>, NodeShapeBuildError>
where
    F: Fn(NodeKind<&str>) -> Option<NodeKindId>,
    E: Fn(&NodeKindRef) -> NodeShapeBuildError,
{
    let mut resolved = Vec::new();
    for kind_ref in &slot.types {
        let node_kind = kind_ref.node_kind();
        if let Some(node_id) = node_id_for_type(node_kind) {
            resolved.push(node_id);
            continue;
        }

        if known_shapes.contains(&node_kind) {
            continue;
        }

        Err(error(kind_ref))?;
    }

    Ok(resolved)
}
