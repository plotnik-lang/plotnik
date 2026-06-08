//! Production grammar type definitions.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU16;

use serde::{Deserialize, Serialize};

use crate::{Cardinality, NodeFieldId, NodeTypeId};

use super::json::GrammarError;
use super::raw::RawGrammar;

pub(super) struct GrammarMetadata {
    pub(super) node_shapes: Vec<NodeShape>,
    pub(super) symbols: Vec<NodeSymbol>,
    pub(super) fields: Vec<FieldSymbol>,
}

#[derive(Debug, Clone)]
pub(super) struct NodeSymbol {
    pub(super) id: u16,
    pub(super) type_name: String,
    pub(super) named: bool,
    pub(super) visible: bool,
    pub(super) supertype: bool,
}

#[derive(Debug, Clone)]
pub(super) struct FieldSymbol {
    pub(super) id: u16,
    pub(super) name: String,
}

/// Tree-sitter grammar plus Plotnik's derived compile-time metadata.
#[derive(Debug, Clone)]
pub struct Grammar {
    name: String,
    node_constraints: HashMap<NodeTypeId, NodeConstraints>,
    extra_node_types: Vec<NodeTypeId>,
    root_node_type: Option<NodeTypeId>,
    named_node_ids: HashMap<String, NodeTypeId>,
    anonymous_node_ids: HashMap<String, NodeTypeId>,
    node_names: HashMap<NodeTypeId, String>,
    field_ids: HashMap<String, NodeFieldId>,
    field_names: HashMap<NodeFieldId, String>,
    supertype_ids: HashSet<NodeTypeId>,
    subtypes: HashMap<NodeTypeId, Vec<NodeTypeId>>,
    fields_by_node: HashMap<NodeTypeId, Vec<String>>,
    all_named_node_kinds: Vec<String>,
    all_anonymous_node_kinds: Vec<String>,
    all_field_names: Vec<String>,
}

impl Grammar {
    /// Build production grammar metadata from a raw source-format grammar.
    pub fn from_raw(raw: &RawGrammar) -> Result<Self, GrammarError> {
        use super::{
            aliases::extract_default_aliases,
            lower::{
                convert_precedence_entry, convert_rule, derive_fields, derive_symbols,
                retain_reachable_rules,
            },
            node_shapes,
            prepared::{ReservedWordContext, Variable, VariableType},
            productions::{expand_repeats, flatten_grammar, process_inlines},
            symbols::resolve_symbols,
            tokens::{expand_tokens, extract_tokens},
            validation::{validate_indirect_recursion, validate_precedences},
        };

        let mut variables = raw
            .rules
            .iter()
            .map(|(name, rule)| Variable {
                name: name.clone(),
                kind: VariableType::Named,
                rule: convert_rule(rule),
            })
            .collect::<Vec<_>>();
        let mut extra_symbols = raw.extras.iter().map(convert_rule).collect::<Vec<_>>();
        let mut expected_conflicts = raw.conflicts.clone();
        let mut precedence_orderings = raw
            .precedences
            .iter()
            .map(|entries| entries.iter().map(convert_precedence_entry).collect())
            .collect::<Vec<_>>();
        let mut external_tokens = raw.externals.iter().map(convert_rule).collect::<Vec<_>>();
        let mut variables_to_inline = raw.inline.clone();
        let mut supertype_symbols = raw.supertypes.clone();
        let word_token = raw.word.clone();
        let mut reserved_words = raw
            .reserved
            .iter()
            .map(|(name, rules)| ReservedWordContext {
                name: name.clone(),
                reserved_words: rules.iter().map(convert_rule).collect(),
            })
            .collect::<Vec<_>>();

        retain_reachable_rules(
            &mut variables,
            &mut extra_symbols,
            &mut expected_conflicts,
            &mut precedence_orderings,
            &mut external_tokens,
            &mut variables_to_inline,
            &mut supertype_symbols,
            word_token.as_deref(),
            &mut reserved_words,
        );
        validate_precedences(&variables, &precedence_orderings)
            .map_err(|error| GrammarError::Analysis(error.to_string()))?;
        validate_indirect_recursion(&variables)
            .map_err(|error| GrammarError::Analysis(error.to_string()))?;

        let resolved_grammar = resolve_symbols(
            &variables,
            &extra_symbols,
            &expected_conflicts,
            &external_tokens,
            &variables_to_inline,
            &supertype_symbols,
            word_token.as_deref(),
            &reserved_words,
        )
        .map_err(|error| GrammarError::Analysis(error.to_string()))?;
        let (syntax_grammar, lexical_grammar) = extract_tokens(resolved_grammar)
            .map_err(|error| GrammarError::Analysis(error.to_string()))?;
        let syntax_grammar = expand_repeats(syntax_grammar);
        let mut syntax_grammar = flatten_grammar(syntax_grammar)
            .map_err(|error| GrammarError::Analysis(error.to_string()))?;
        let lexical_grammar = expand_tokens(lexical_grammar)
            .map_err(|error| GrammarError::Analysis(error.to_string()))?;
        let aliases = extract_default_aliases(&mut syntax_grammar, &lexical_grammar);
        let inlines = process_inlines(&syntax_grammar, &lexical_grammar)
            .map_err(|error| GrammarError::Analysis(error.to_string()))?;

        let variable_info =
            node_shapes::get_variable_info(&syntax_grammar, &lexical_grammar, &aliases)
                .map_err(|error| GrammarError::Analysis(error.to_string()))?;
        let node_shapes = node_shapes::generate_node_shapes(
            &syntax_grammar,
            &lexical_grammar,
            &aliases,
            &variable_info,
        )
        .map_err(|error| GrammarError::Analysis(error.to_string()))?;
        let metadata = GrammarMetadata {
            node_shapes,
            symbols: derive_symbols(&syntax_grammar, &lexical_grammar, &inlines, &aliases),
            fields: derive_fields(&syntax_grammar, &inlines, &variable_info),
        };

        Self::from_metadata(raw.name.clone(), metadata).map_err(GrammarError::Analysis)
    }

    fn from_metadata(name: String, metadata: GrammarMetadata) -> Result<Self, String> {
        let mut node_ids = HashMap::<(String, bool), NodeTypeId>::new();
        let mut named_node_ids = HashMap::new();
        let mut anonymous_node_ids = HashMap::new();
        let mut node_names = HashMap::new();
        let mut supertype_ids = HashSet::new();

        for symbol in &metadata.symbols {
            let node_id = node_type_id(symbol.id);
            node_names.insert(node_id, symbol.type_name.clone());

            if symbol.supertype {
                supertype_ids.insert(node_id);
            }

            if !symbol.visible && !symbol.supertype {
                continue;
            }

            node_ids
                .entry((symbol.type_name.clone(), symbol.named))
                .or_insert(node_id);

            if symbol.named {
                named_node_ids
                    .entry(symbol.type_name.clone())
                    .or_insert(node_id);
            } else {
                anonymous_node_ids
                    .entry(symbol.type_name.clone())
                    .or_insert(node_id);
            }
        }

        let mut field_ids = HashMap::new();
        let mut field_names = HashMap::new();
        for field in &metadata.fields {
            let field_id = node_field_id(field.id);
            field_ids.insert(field.name.clone(), field_id);
            field_names.insert(field_id, field.name.clone());
        }

        let (node_constraints, extra_node_types, root_node_type) = build_node_constraints(
            &metadata.node_shapes,
            |name, named| node_ids.get(&(name.to_string(), named)).copied(),
            |name| field_ids.get(name).copied(),
        )
        .map_err(format_node_shape_error)?;

        let mut subtypes = HashMap::new();
        for shape in &metadata.node_shapes {
            let Some(shape_subtypes) = &shape.subtypes else {
                continue;
            };
            let Some(supertype) = node_ids.get(&(shape.type_name.clone(), shape.named)) else {
                continue;
            };

            let resolved = shape_subtypes
                .iter()
                .filter_map(|subtype| {
                    node_ids
                        .get(&(subtype.type_name.clone(), subtype.named))
                        .copied()
                })
                .collect::<Vec<_>>();
            subtypes.insert(*supertype, resolved);
        }

        let mut fields_by_node = HashMap::new();
        for shape in &metadata.node_shapes {
            let Some(node_id) = node_ids.get(&(shape.type_name.clone(), shape.named)) else {
                continue;
            };
            let mut fields = shape.fields.keys().cloned().collect::<Vec<_>>();
            fields.sort();
            fields_by_node.insert(*node_id, fields);
        }

        let mut all_named_node_kinds = named_node_ids.keys().cloned().collect::<Vec<_>>();
        all_named_node_kinds.sort();

        let mut all_anonymous_node_kinds = anonymous_node_ids.keys().cloned().collect::<Vec<_>>();
        all_anonymous_node_kinds.sort();

        let mut all_field_names = field_ids.keys().cloned().collect::<Vec<_>>();
        all_field_names.sort();

        Ok(Self {
            name,
            node_constraints,
            extra_node_types,
            root_node_type,
            named_node_ids,
            anonymous_node_ids,
            node_names,
            field_ids,
            field_names,
            supertype_ids,
            subtypes,
            fields_by_node,
            all_named_node_kinds,
            all_anonymous_node_kinds,
            all_field_names,
        })
    }

    /// Grammar name (e.g., "javascript", "rust").
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Resolve a named node kind to its tree-sitter ABI id.
    pub fn resolve_named_node(&self, kind: &str) -> Option<NodeTypeId> {
        self.named_node_ids.get(kind).copied()
    }

    /// Resolve an anonymous node kind to its tree-sitter ABI id.
    pub fn resolve_anonymous_node(&self, kind: &str) -> Option<NodeTypeId> {
        self.anonymous_node_ids.get(kind).copied()
    }

    /// Resolve a field name to its tree-sitter ABI id.
    pub fn resolve_field(&self, field: &str) -> Option<NodeFieldId> {
        self.field_ids.get(field).copied()
    }

    /// Human-readable node kind for diagnostics/debugging.
    pub fn node_type_name(&self, node_type_id: NodeTypeId) -> Option<&str> {
        self.node_names.get(&node_type_id).map(String::as_str)
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

    pub fn fields_for_node_type(&self, node_type_id: NodeTypeId) -> Vec<&str> {
        self.fields_by_node
            .get(&node_type_id)
            .map(|fields| fields.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    pub fn is_supertype(&self, node_type_id: NodeTypeId) -> bool {
        self.supertype_ids.contains(&node_type_id)
    }

    pub fn subtypes(&self, supertype: NodeTypeId) -> &[NodeTypeId] {
        self.subtypes
            .get(&supertype)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn root(&self) -> Option<NodeTypeId> {
        self.root_node_type
    }

    pub fn is_extra(&self, node_type_id: NodeTypeId) -> bool {
        self.extra_node_types.contains(&node_type_id)
    }

    pub fn has_field(&self, node_type_id: NodeTypeId, node_field_id: NodeFieldId) -> bool {
        self.node_constraints
            .get(&node_type_id)
            .is_some_and(|constraints| constraints.fields.contains_key(&node_field_id))
    }

    pub fn field_cardinality(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
    ) -> Option<Cardinality> {
        self.field_constraints(node_type_id, node_field_id)
            .map(|field| field.cardinality)
    }

    pub fn valid_field_types(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
    ) -> &[NodeTypeId] {
        self.field_constraints(node_type_id, node_field_id)
            .map(|field| field.valid_types.as_slice())
            .unwrap_or(&[])
    }

    pub fn is_valid_field_type(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
        child: NodeTypeId,
    ) -> bool {
        self.valid_field_types(node_type_id, node_field_id)
            .contains(&child)
    }

    pub fn children_cardinality(&self, node_type_id: NodeTypeId) -> Option<Cardinality> {
        self.children_constraints(node_type_id)
            .map(|children| children.cardinality)
    }

    pub fn valid_child_types(&self, node_type_id: NodeTypeId) -> &[NodeTypeId] {
        self.children_constraints(node_type_id)
            .map(|children| children.valid_types.as_slice())
            .unwrap_or(&[])
    }

    pub fn is_valid_child_type(&self, node_type_id: NodeTypeId, child: NodeTypeId) -> bool {
        self.valid_child_types(node_type_id).contains(&child)
    }

    fn field_constraints(
        &self,
        node_type_id: NodeTypeId,
        field_id: NodeFieldId,
    ) -> Option<&FieldConstraints> {
        self.node_constraints_for(node_type_id)
            .fields
            .get(&field_id)
    }

    fn children_constraints(&self, node_type_id: NodeTypeId) -> Option<&ChildrenConstraints> {
        self.node_constraints_for(node_type_id).children.as_ref()
    }

    fn node_constraints_for(&self, node_type_id: NodeTypeId) -> &NodeConstraints {
        self.node_constraints.get(&node_type_id).unwrap_or_else(|| {
            panic!(
                "Grammar: node type id {node_type_id} not found \
                     (grammar metadata must match linked node ids)"
            )
        })
    }
}

fn node_type_id(id: u16) -> NodeTypeId {
    NonZeroU16::new(id).expect("lowered node symbol id must be non-zero in production grammar")
}

fn node_field_id(id: u16) -> NodeFieldId {
    NonZeroU16::new(id).expect("lowered field symbol id must be non-zero in production grammar")
}

fn format_node_shape_error(error: NodeShapeBuildError) -> String {
    error.to_string()
}

/// Grammar-derived metadata for a syntax node kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct NodeShape {
    #[serde(rename = "type")]
    pub(crate) type_name: String,
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

/// Cardinality constraints for a field or children slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct NodeSlot {
    pub(crate) multiple: bool,
    pub(crate) required: bool,
    pub(crate) types: Vec<NodeKindRef>,
}

/// Reference to a node kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct NodeKindRef {
    #[serde(rename = "type")]
    pub(crate) type_name: String,
    pub(crate) named: bool,
}

/// Error while resolving grammar-derived node shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NodeShapeBuildError {
    UnknownField {
        node_kind: String,
        field: String,
    },
    UnknownFieldType {
        node_kind: String,
        field: String,
        kind: String,
        named: bool,
    },
    UnknownChildType {
        node_kind: String,
        kind: String,
        named: bool,
    },
}

impl std::fmt::Display for NodeShapeBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownField { node_kind, field } => {
                write!(f, "unknown field {field:?} on node kind {node_kind:?}")
            }
            Self::UnknownFieldType {
                node_kind,
                field,
                kind,
                named,
            } => write!(
                f,
                "unknown field type {kind:?} (named: {named}) for field {field:?} on node kind {node_kind:?}"
            ),
            Self::UnknownChildType {
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

/// Field constraints for a named field on a node type.
#[derive(Debug, Clone)]
pub(crate) struct FieldConstraints {
    pub(crate) cardinality: Cardinality,
    pub(crate) valid_types: Vec<NodeTypeId>,
}

/// Children constraints for non-field children on a node type.
#[derive(Debug, Clone)]
pub(crate) struct ChildrenConstraints {
    pub(crate) cardinality: Cardinality,
    pub(crate) valid_types: Vec<NodeTypeId>,
}

/// Constraints for a concrete node type.
#[derive(Debug, Clone)]
pub(crate) struct NodeConstraints {
    pub(crate) fields: HashMap<NodeFieldId, FieldConstraints>,
    pub(crate) children: Option<ChildrenConstraints>,
}

type NodeConstraintBuild = (
    HashMap<NodeTypeId, NodeConstraints>,
    Vec<NodeTypeId>,
    Option<NodeTypeId>,
);

pub(crate) fn build_node_constraints<F, G>(
    node_shapes: &[NodeShape],
    node_id_for_name: F,
    field_id_for_name: G,
) -> Result<NodeConstraintBuild, NodeShapeBuildError>
where
    F: Fn(&str, bool) -> Option<NodeTypeId>,
    G: Fn(&str) -> Option<NodeFieldId>,
{
    let mut node_constraints = HashMap::new();
    let mut extra_node_types = Vec::new();
    let mut root_node_type = None;
    let known_shapes = node_shapes
        .iter()
        .map(|shape| (shape.type_name.as_str(), shape.named))
        .collect::<HashSet<_>>();

    for shape in node_shapes {
        let Some(node_id) = node_id_for_name(&shape.type_name, shape.named) else {
            continue;
        };

        if shape.root {
            root_node_type = Some(node_id);
        }

        if shape.extra {
            extra_node_types.push(node_id);
        }

        let fields =
            build_field_constraints(shape, &known_shapes, &node_id_for_name, &field_id_for_name)?;
        let children = build_children_constraints(shape, &known_shapes, &node_id_for_name)?;

        node_constraints.insert(node_id, NodeConstraints { fields, children });
    }

    Ok((node_constraints, extra_node_types, root_node_type))
}

fn build_field_constraints<F, G>(
    shape: &NodeShape,
    known_shapes: &HashSet<(&str, bool)>,
    node_id_for_name: &F,
    field_id_for_name: &G,
) -> Result<HashMap<NodeFieldId, FieldConstraints>, NodeShapeBuildError>
where
    F: Fn(&str, bool) -> Option<NodeTypeId>,
    G: Fn(&str) -> Option<NodeFieldId>,
{
    let mut fields = HashMap::new();
    for (field_name, slot) in &shape.fields {
        let field_id =
            field_id_for_name(field_name).ok_or_else(|| NodeShapeBuildError::UnknownField {
                node_kind: shape.type_name.clone(),
                field: field_name.clone(),
            })?;

        let valid_types = resolve_slot_types(slot, known_shapes, node_id_for_name, |kind_ref| {
            NodeShapeBuildError::UnknownFieldType {
                node_kind: shape.type_name.clone(),
                field: field_name.clone(),
                kind: kind_ref.type_name.clone(),
                named: kind_ref.named,
            }
        })?;

        fields.insert(
            field_id,
            FieldConstraints {
                cardinality: Cardinality {
                    multiple: slot.multiple,
                    required: slot.required,
                },
                valid_types,
            },
        );
    }

    Ok(fields)
}

fn build_children_constraints<F>(
    shape: &NodeShape,
    known_shapes: &HashSet<(&str, bool)>,
    node_id_for_name: &F,
) -> Result<Option<ChildrenConstraints>, NodeShapeBuildError>
where
    F: Fn(&str, bool) -> Option<NodeTypeId>,
{
    shape
        .children
        .as_ref()
        .map(|slot| {
            let valid_types =
                resolve_slot_types(slot, known_shapes, node_id_for_name, |kind_ref| {
                    NodeShapeBuildError::UnknownChildType {
                        node_kind: shape.type_name.clone(),
                        kind: kind_ref.type_name.clone(),
                        named: kind_ref.named,
                    }
                })?;

            Ok(ChildrenConstraints {
                cardinality: Cardinality {
                    multiple: slot.multiple,
                    required: slot.required,
                },
                valid_types,
            })
        })
        .transpose()
}

fn resolve_slot_types<F, E>(
    slot: &NodeSlot,
    known_shapes: &HashSet<(&str, bool)>,
    node_id_for_name: &F,
    error: E,
) -> Result<Vec<NodeTypeId>, NodeShapeBuildError>
where
    F: Fn(&str, bool) -> Option<NodeTypeId>,
    E: Fn(&NodeKindRef) -> NodeShapeBuildError,
{
    let mut resolved = Vec::new();
    for kind_ref in &slot.types {
        if let Some(node_id) = node_id_for_name(&kind_ref.type_name, kind_ref.named) {
            resolved.push(node_id);
            continue;
        }

        if known_shapes.contains(&(kind_ref.type_name.as_str(), kind_ref.named)) {
            continue;
        }

        Err(error(kind_ref))?;
    }

    Ok(resolved)
}
