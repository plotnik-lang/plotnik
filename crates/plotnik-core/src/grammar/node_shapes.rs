use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::types::{NodeKindRef, NodeShape, NodeSlot};

use super::{
    prepared::{LexicalGrammar, ProductionStep, SyntaxGrammar, VariableType},
    rules::{Alias, AliasMap, Symbol, SymbolType},
};

const START_RULE_INDEX: usize = 0;

/// Borrowed view of the grammar tables that node-shape derivation threads
/// together: the syntax grammar, the lexical grammar, and the default aliases.
#[derive(Clone, Copy)]
pub struct GrammarContext<'a> {
    pub syntax: &'a SyntaxGrammar,
    pub lexical: &'a LexicalGrammar,
    pub aliases: &'a AliasMap,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChildType {
    Normal(Symbol),
    Aliased(Alias),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldInfo {
    pub quantity: ChildQuantity,
    pub types: Vec<ChildType>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VariableInfo {
    pub fields: FxHashMap<String, FieldInfo>,
    pub children: FieldInfo,
    pub children_without_fields: FieldInfo,
    pub has_multi_step_production: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq, Default, PartialOrd, Ord)]
pub struct NodeShapeJSON {
    #[serde(rename = "type")]
    kind: String,
    named: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    root: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    extra: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    fields: Option<BTreeMap<String, FieldInfoJSON>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    children: Option<FieldInfoJSON>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subtypes: Option<Vec<NodeTypeJSON>>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeTypeJSON {
    #[serde(rename = "type")]
    kind: String,
    named: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct FieldInfoJSON {
    multiple: bool,
    required: bool,
    types: Vec<NodeTypeJSON>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChildQuantity {
    exists: bool,
    required: bool,
    multiple: bool,
}

impl Default for FieldInfoJSON {
    fn default() -> Self {
        Self {
            multiple: false,
            required: true,
            types: Vec::new(),
        }
    }
}

impl From<NodeTypeJSON> for NodeKindRef {
    fn from(value: NodeTypeJSON) -> Self {
        Self {
            type_name: value.kind,
            named: value.named,
        }
    }
}

impl From<FieldInfoJSON> for NodeSlot {
    fn from(value: FieldInfoJSON) -> Self {
        Self {
            multiple: value.multiple,
            required: value.required,
            types: value.types.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<NodeShapeJSON> for NodeShape {
    fn from(value: NodeShapeJSON) -> Self {
        Self {
            type_name: value.kind,
            named: value.named,
            root: value.root,
            extra: value.extra,
            fields: value
                .fields
                .unwrap_or_default()
                .into_iter()
                .map(|(name, slot)| (name, slot.into()))
                .collect(),
            children: value.children.map(Into::into),
            subtypes: value
                .subtypes
                .map(|types| types.into_iter().map(Into::into).collect()),
        }
    }
}

impl Default for ChildQuantity {
    fn default() -> Self {
        Self::one()
    }
}

impl ChildQuantity {
    #[must_use]
    const fn zero() -> Self {
        Self {
            exists: false,
            required: false,
            multiple: false,
        }
    }

    #[must_use]
    const fn one() -> Self {
        Self {
            exists: true,
            required: true,
            multiple: false,
        }
    }

    const fn append(&mut self, other: Self) {
        if other.exists {
            if self.exists || other.multiple {
                self.multiple = true;
            }
            if other.required {
                self.required = true;
            }
            self.exists = true;
        }
    }

    const fn union(&mut self, other: Self) -> bool {
        let mut result = false;
        if !self.exists && other.exists {
            result = true;
            self.exists = true;
        }
        if self.required && !other.required {
            result = true;
            self.required = false;
        }
        if !self.multiple && other.multiple {
            result = true;
            self.multiple = true;
        }
        result
    }
}

pub type VariableInfoResult<T> = Result<T, VariableInfoError>;

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum VariableInfoError {
    #[error(
        "Grammar error: Supertype symbols must always have a single visible child, but `{0}` can have multiple"
    )]
    InvalidSupertype(String),
}

/// Compute a summary of the public-facing structure of each variable in the
/// grammar. Each variable in the grammar corresponds to a distinct public-facing
/// node type.
///
/// The information collected about each node type `N` is:
/// 1. `child_types` - The types of visible children that can appear within `N`.
/// 2. `fields` - The fields that `N` can have. Data regarding each field:
///    * `types` - The types of visible children the field can contain.
///    * `optional` - Do `N` nodes always have this field?
///    * `multiple` - Can `N` nodes have multiple children for this field?
/// 3. `children_without_fields` - The *other* named children of `N` that are not associated with
///    fields. Data regarding these children:
///    * `types` - The types of named children with no field.
///    * `optional` - Do `N` nodes always have at least one named child with no field?
///    * `multiple` - Can `N` nodes have multiple named children with no field?
///
/// Each summary must account for some indirect factors:
/// 1. hidden nodes. When a parent node `N` has a hidden child `C`, the visible children of `C`
///    *appear* to be direct children of `N`.
/// 2. aliases. If a parent node type `M` is aliased as some other type `N`, then nodes which
///    *appear* to have type `N` may have internal structure based on `M`.
pub fn get_variable_info(ctx: GrammarContext<'_>) -> VariableInfoResult<Vec<VariableInfo>> {
    let GrammarContext {
        syntax: syntax_grammar,
        lexical: lexical_grammar,
        aliases: default_aliases,
    } = ctx;

    let child_type_is_visible = |t: &ChildType| {
        variable_type_for_child_type(t, syntax_grammar, lexical_grammar).is_visible()
    };

    let child_type_is_named = |t: &ChildType| {
        variable_type_for_child_type(t, syntax_grammar, lexical_grammar) == VariableType::Named
    };

    // Each variable's summary can depend on the summaries of other hidden variables,
    // and variables can have mutually recursive structure. So we compute the summaries
    // iteratively, in a loop that terminates only when no more changes are possible.
    let mut did_change = true;
    let mut all_initialized = false;
    let mut result = vec![VariableInfo::default(); syntax_grammar.variables.len()];
    while did_change {
        did_change = false;

        for (i, variable) in syntax_grammar.variables.iter().enumerate() {
            let mut variable_info = result[i].clone();

            // Examine each of the variable's productions. The variable's child types can be
            // immediately combined across all productions, but the child quantities must be
            // recorded separately for each production.
            for production in &variable.productions {
                let mut production_field_quantities = FxHashMap::default();
                let mut production_children_quantity = ChildQuantity::zero();
                let mut production_children_without_fields_quantity = ChildQuantity::zero();
                let mut production_has_uninitialized_invisible_children = false;

                if production.steps.len() > 1 {
                    variable_info.has_multi_step_production = true;
                }

                for step in &production.steps {
                    let child_symbol = step.symbol;
                    let child_type = effective_step_alias(step, default_aliases)
                        .cloned()
                        .map_or(ChildType::Normal(child_symbol), ChildType::Aliased);

                    let child_is_hidden = !child_type_is_visible(&child_type)
                        && !syntax_grammar.supertype_symbols.contains(&child_symbol);

                    // Maintain the set of all child types for this variable, and the quantity of
                    // visible children in this production.
                    did_change |=
                        extend_sorted(&mut variable_info.children.types, Some(&child_type));
                    if !child_is_hidden {
                        production_children_quantity.append(ChildQuantity::one());
                    }

                    // Maintain the set of child types associated with each field, and the quantity
                    // of children associated with each field in this production.
                    if let Some(field_name) = &step.field_name {
                        let field_info = variable_info
                            .fields
                            .entry(field_name.clone())
                            .or_insert_with(FieldInfo::default);
                        did_change |= extend_sorted(&mut field_info.types, Some(&child_type));

                        let production_field_quantity = production_field_quantities
                            .entry(field_name)
                            .or_insert_with(ChildQuantity::zero);

                        // Inherit the types and quantities of hidden children associated with
                        // fields.
                        if child_is_hidden && child_symbol.is_non_terminal() {
                            let child_variable_info = &result[child_symbol.index];
                            did_change |= extend_sorted(
                                &mut field_info.types,
                                &child_variable_info.children.types,
                            );
                            production_field_quantity.append(child_variable_info.children.quantity);
                        } else {
                            production_field_quantity.append(ChildQuantity::one());
                        }
                    }
                    // Maintain the set of named children without fields within this variable.
                    else if child_type_is_named(&child_type) {
                        production_children_without_fields_quantity.append(ChildQuantity::one());
                        did_change |= extend_sorted(
                            &mut variable_info.children_without_fields.types,
                            Some(&child_type),
                        );
                    }

                    // Inherit all child information from hidden children.
                    if child_is_hidden && child_symbol.is_non_terminal() {
                        let child_variable_info = &result[child_symbol.index];

                        // If a hidden child can have multiple children, then its parent node can
                        // appear to have multiple children.
                        if child_variable_info.has_multi_step_production {
                            variable_info.has_multi_step_production = true;
                        }

                        // If a hidden child has fields, then the parent node can appear to have
                        // those same fields.
                        for (field_name, child_field_info) in &child_variable_info.fields {
                            production_field_quantities
                                .entry(field_name)
                                .or_insert_with(ChildQuantity::zero)
                                .append(child_field_info.quantity);
                            did_change |= extend_sorted(
                                &mut variable_info
                                    .fields
                                    .entry(field_name.clone())
                                    .or_insert_with(FieldInfo::default)
                                    .types,
                                &child_field_info.types,
                            );
                        }

                        // If a hidden child has children, then the parent node can appear to have
                        // those same children.
                        production_children_quantity.append(child_variable_info.children.quantity);
                        did_change |= extend_sorted(
                            &mut variable_info.children.types,
                            &child_variable_info.children.types,
                        );

                        // If a hidden child can have named children without fields, then the parent
                        // node can appear to have those same children.
                        if step.field_name.is_none() {
                            let grandchildren_info = &child_variable_info.children_without_fields;
                            if !grandchildren_info.types.is_empty() {
                                production_children_without_fields_quantity
                                    .append(child_variable_info.children_without_fields.quantity);
                                did_change |= extend_sorted(
                                    &mut variable_info.children_without_fields.types,
                                    &child_variable_info.children_without_fields.types,
                                );
                            }
                        }
                    }

                    // Note whether or not this production contains children whose summaries
                    // have not yet been computed.
                    if child_symbol.index >= i && !all_initialized {
                        production_has_uninitialized_invisible_children = true;
                    }
                }

                // If this production's children all have had their summaries initialized,
                // then expand the quantity information with all of the possibilities introduced
                // by this production.
                if !production_has_uninitialized_invisible_children {
                    did_change |= variable_info
                        .children
                        .quantity
                        .union(production_children_quantity);

                    did_change |= variable_info
                        .children_without_fields
                        .quantity
                        .union(production_children_without_fields_quantity);

                    for (field_name, info) in &mut variable_info.fields {
                        did_change |= info.quantity.union(
                            production_field_quantities
                                .get(field_name)
                                .copied()
                                .unwrap_or_else(ChildQuantity::zero),
                        );
                    }
                }
            }

            result[i] = variable_info;
        }

        all_initialized = true;
    }

    validate_supertype_info(&result, syntax_grammar)?;
    retain_visible_child_types(&mut result, syntax_grammar, &child_type_is_visible);

    Ok(result)
}

fn validate_supertype_info(
    variable_info: &[VariableInfo],
    syntax_grammar: &SyntaxGrammar,
) -> VariableInfoResult<()> {
    for supertype_symbol in &syntax_grammar.supertype_symbols {
        if variable_info[supertype_symbol.index].has_multi_step_production {
            let variable = &syntax_grammar.variables[supertype_symbol.index];
            Err(VariableInfoError::InvalidSupertype(variable.name.clone()))?;
        }
    }

    Ok(())
}

fn retain_visible_child_types(
    variable_info: &mut [VariableInfo],
    syntax_grammar: &SyntaxGrammar,
    child_type_is_visible: &impl Fn(&ChildType) -> bool,
) {
    // Update all of the node type lists to eliminate hidden nodes.
    for supertype_symbol in &syntax_grammar.supertype_symbols {
        variable_info[supertype_symbol.index]
            .children
            .types
            .retain(child_type_is_visible);
    }
    for variable_info in variable_info {
        for field_info in variable_info.fields.values_mut() {
            field_info.types.retain(child_type_is_visible);
        }
        variable_info.fields.retain(|_, v| !v.types.is_empty());
        variable_info
            .children_without_fields
            .types
            .retain(child_type_is_visible);
    }
}

pub type SuperTypeCycleResult<T> = Result<T, SuperTypeCycleError>;

#[derive(Debug, Error, Serialize, Deserialize)]
pub struct SuperTypeCycleError {
    items: Vec<String>,
}

impl std::fmt::Display for SuperTypeCycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Dependency cycle detected in node types:")?;
        for (i, item) in self.items.iter().enumerate() {
            write!(f, " {item}")?;
            if i < self.items.len() - 1 {
                write!(f, ",")?;
            }
        }

        Ok(())
    }
}

pub fn generate_node_shapes_json(
    ctx: GrammarContext<'_>,
    variable_info: &[VariableInfo],
) -> SuperTypeCycleResult<Vec<NodeShapeJSON>> {
    let GrammarContext {
        syntax: syntax_grammar,
        lexical: lexical_grammar,
        aliases: default_aliases,
    } = ctx;

    let mut node_shapes_json = BTreeMap::new();

    let populate_field_info_json = |json: &mut FieldInfoJSON, info: &FieldInfo| {
        if info.types.is_empty() {
            json.required = false;
        } else {
            json.multiple |= info.quantity.multiple;
            json.required &= info.quantity.required;
            json.types.extend(
                info.types
                    .iter()
                    .map(|child_type| child_type_to_node_type(child_type, ctx)),
            );
            json.types.sort_unstable();
            json.types.dedup();
        }
    };

    let aliases_by_symbol = get_aliases_by_symbol(syntax_grammar, default_aliases);

    let extra_names = collect_extra_names(syntax_grammar, lexical_grammar, &aliases_by_symbol);

    let mut subtype_map = Vec::new();
    for (i, info) in variable_info.iter().enumerate() {
        let symbol = Symbol::non_terminal(i);
        let variable = &syntax_grammar.variables[i];
        if syntax_grammar.supertype_symbols.contains(&symbol) {
            let node_shape_json = node_shapes_json
                .entry(variable.name.clone())
                .or_insert_with(|| NodeShapeJSON {
                    kind: variable.name.clone(),
                    named: true,
                    root: false,
                    extra: extra_names.contains(variable.name.as_str()),
                    fields: None,
                    children: None,
                    subtypes: None,
                });
            let mut subtypes = info
                .children
                .types
                .iter()
                .map(|child_type| child_type_to_node_type(child_type, ctx))
                .collect::<Vec<_>>();
            subtypes.sort_unstable();
            subtypes.dedup();
            let supertype = NodeTypeJSON {
                kind: node_shape_json.kind.clone(),
                named: true,
            };

            // We only add to the subtype map if there are visible subtypes.
            // A supertype may have zero subtypes if its children are all
            // hidden (e.g., wrapping a hidden external token).
            if !subtypes.is_empty() {
                subtype_map.push((supertype, subtypes.clone()));
            }
            node_shape_json.subtypes = Some(subtypes);
        } else if !syntax_grammar.variables_to_inline.contains(&symbol) {
            // If a rule is aliased under multiple names, then its information
            // contributes to multiple entries in the final JSON.
            for alias in aliases_by_symbol.get(&symbol).unwrap_or(&BTreeSet::new()) {
                let (kind, is_named) = if let Some(alias) = alias {
                    (&alias.value, alias.is_named)
                } else if variable.kind.is_visible() {
                    (&variable.name, variable.kind == VariableType::Named)
                } else {
                    continue;
                };

                // There may already be an entry with this name, because multiple
                // rules may be aliased with the same name.
                let mut node_type_existed = true;
                let node_shape_json = node_shapes_json.entry(kind.clone()).or_insert_with(|| {
                    node_type_existed = false;
                    NodeShapeJSON {
                        kind: kind.clone(),
                        named: is_named,
                        root: i == START_RULE_INDEX,
                        extra: extra_names.contains(kind.as_str()),
                        fields: Some(BTreeMap::new()),
                        children: None,
                        subtypes: None,
                    }
                });

                let fields_json = node_shape_json.fields.as_mut().unwrap();
                for (new_field, field_info) in &info.fields {
                    let field_json = fields_json.entry(new_field.clone()).or_insert_with(|| {
                        // If another rule is aliased with the same name, and does *not* have this
                        // field, then this field cannot be required.
                        let mut field_json = FieldInfoJSON::default();
                        if node_type_existed {
                            field_json.required = false;
                        }
                        field_json
                    });
                    populate_field_info_json(field_json, field_info);
                }

                // If another rule is aliased with the same name, any fields that aren't present in
                // this cannot be required.
                for (existing_field, field_json) in fields_json.iter_mut() {
                    if !info.fields.contains_key(existing_field) {
                        field_json.required = false;
                    }
                }

                populate_field_info_json(
                    node_shape_json
                        .children
                        .get_or_insert_with(FieldInfoJSON::default),
                    &info.children_without_fields,
                );
            }
        }
    }

    sort_subtypes_topologically(&mut subtype_map)?;

    for node_shape_json in node_shapes_json.values_mut() {
        if node_shape_json
            .children
            .as_ref()
            .is_some_and(|c| c.types.is_empty())
        {
            node_shape_json.children = None;
        }

        if let Some(children) = &mut node_shape_json.children {
            process_supertypes(children, &subtype_map);
        }
        if let Some(fields) = &mut node_shape_json.fields {
            for field_info in fields.values_mut() {
                process_supertypes(field_info, &subtype_map);
            }
        }
    }

    let anonymous_node_shapes = add_token_node_shapes(
        &mut node_shapes_json,
        syntax_grammar,
        lexical_grammar,
        &aliases_by_symbol,
        &extra_names,
    );

    let mut result = node_shapes_json
        .into_iter()
        .map(|e| e.1)
        .collect::<Vec<_>>();
    result.extend(anonymous_node_shapes);
    let is_leaf = |node: &NodeShapeJSON| node.children.is_none() && node.fields.is_none();
    // Keep output deterministic and close to Tree-sitter's node metadata: supertypes first,
    // structured concrete nodes next, leaf tokens last, then stable scalar tie-breakers.
    result.sort_unstable_by(|a, b| {
        b.subtypes
            .is_some()
            .cmp(&a.subtypes.is_some())
            .then_with(|| is_leaf(a).cmp(&is_leaf(b)))
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.named.cmp(&b.named))
            .then_with(|| a.root.cmp(&b.root))
            .then_with(|| a.extra.cmp(&b.extra))
    });
    result.dedup();
    Ok(result)
}

pub fn generate_node_shapes(
    ctx: GrammarContext<'_>,
    variable_info: &[VariableInfo],
) -> SuperTypeCycleResult<Vec<NodeShape>> {
    generate_node_shapes_json(ctx, variable_info)
        .map(|shapes| shapes.into_iter().map(Into::into).collect())
}

fn get_aliases_by_symbol(
    syntax_grammar: &SyntaxGrammar,
    default_aliases: &AliasMap,
) -> FxHashMap<Symbol, BTreeSet<Option<Alias>>> {
    let mut aliases_by_symbol = FxHashMap::default();
    for (symbol, alias) in default_aliases {
        aliases_by_symbol.insert(*symbol, {
            let mut aliases = BTreeSet::new();
            aliases.insert(Some(alias.clone()));
            aliases
        });
    }
    for extra_symbol in &syntax_grammar.extra_symbols {
        if !default_aliases.contains_key(extra_symbol) {
            aliases_by_symbol
                .entry(*extra_symbol)
                .or_insert_with(BTreeSet::new)
                .insert(None);
        }
    }
    for variable in &syntax_grammar.variables {
        for production in &variable.productions {
            for step in &production.steps {
                aliases_by_symbol
                    .entry(step.symbol)
                    .or_insert_with(BTreeSet::new)
                    .insert(effective_step_alias(step, default_aliases).cloned());
            }
        }
    }
    aliases_by_symbol.insert(
        Symbol::non_terminal(START_RULE_INDEX),
        std::iter::once(&None).cloned().collect(),
    );
    aliases_by_symbol
}

fn effective_step_alias<'a>(
    step: &'a ProductionStep,
    default_aliases: &'a AliasMap,
) -> Option<&'a Alias> {
    step.alias
        .as_ref()
        .or_else(|| default_aliases.get(&step.symbol))
}

fn child_type_to_node_type(child_type: &ChildType, ctx: GrammarContext<'_>) -> NodeTypeJSON {
    match child_type {
        ChildType::Aliased(alias) => alias_to_node_type(alias),
        ChildType::Normal(symbol) => ctx.aliases.get(symbol).map_or_else(
            || symbol_to_node_type(*symbol, ctx.syntax, ctx.lexical),
            alias_to_node_type,
        ),
    }
}

fn alias_to_node_type(alias: &Alias) -> NodeTypeJSON {
    NodeTypeJSON {
        kind: alias.value.clone(),
        named: alias.is_named,
    }
}

fn symbol_to_node_type(
    symbol: Symbol,
    syntax_grammar: &SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
) -> NodeTypeJSON {
    let (name, kind) = symbol_node_metadata(symbol, syntax_grammar, lexical_grammar);
    NodeTypeJSON {
        kind: name.to_string(),
        named: kind != VariableType::Anonymous,
    }
}

fn collect_extra_names<'a>(
    syntax_grammar: &'a SyntaxGrammar,
    lexical_grammar: &'a LexicalGrammar,
    aliases_by_symbol: &'a FxHashMap<Symbol, BTreeSet<Option<Alias>>>,
) -> FxHashSet<&'a str> {
    let mut names = FxHashSet::default();
    for symbol in &syntax_grammar.extra_symbols {
        let Some(aliases) = aliases_by_symbol.get(symbol) else {
            continue;
        };

        for alias in aliases {
            let name = alias.as_ref().map_or_else(
                || symbol_node_metadata(*symbol, syntax_grammar, lexical_grammar).0,
                |alias| alias.value.as_str(),
            );
            names.insert(name);
        }
    }
    names
}

fn add_token_node_shapes(
    node_shapes_json: &mut BTreeMap<String, NodeShapeJSON>,
    syntax_grammar: &SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
    aliases_by_symbol: &FxHashMap<Symbol, BTreeSet<Option<Alias>>>,
    extra_names: &FxHashSet<&str>,
) -> Vec<NodeShapeJSON> {
    let empty = BTreeSet::new();
    let mut anonymous_node_shapes = Vec::new();

    for (i, variable) in lexical_grammar.variables.iter().enumerate() {
        for alias in aliases_by_symbol
            .get(&Symbol::terminal(i))
            .unwrap_or(&empty)
        {
            let (name, kind) = alias.as_ref().map_or_else(
                || (variable.name.as_str(), variable.kind),
                |alias| (alias.value.as_str(), alias.kind()),
            );
            add_token_node_shape(
                node_shapes_json,
                &mut anonymous_node_shapes,
                name,
                kind,
                extra_names,
            );
        }
    }

    for (i, token) in syntax_grammar.external_tokens.iter().enumerate() {
        for alias in aliases_by_symbol
            .get(&Symbol::external(i))
            .unwrap_or(&empty)
        {
            let (name, kind) = alias.as_ref().map_or_else(
                || (token.name.as_str(), token.kind),
                |alias| (alias.value.as_str(), alias.kind()),
            );
            add_token_node_shape(
                node_shapes_json,
                &mut anonymous_node_shapes,
                name,
                kind,
                extra_names,
            );
        }
    }

    anonymous_node_shapes
}

fn add_token_node_shape(
    node_shapes_json: &mut BTreeMap<String, NodeShapeJSON>,
    anonymous_node_shapes: &mut Vec<NodeShapeJSON>,
    name: &str,
    kind: VariableType,
    extra_names: &FxHashSet<&str>,
) {
    match kind {
        VariableType::Named => {
            let node_shape_json =
                node_shapes_json
                    .entry(name.to_string())
                    .or_insert_with(|| NodeShapeJSON {
                        kind: name.to_string(),
                        named: true,
                        root: false,
                        extra: extra_names.contains(name),
                        fields: None,
                        children: None,
                        subtypes: None,
                    });
            if let Some(children) = &mut node_shape_json.children {
                children.required = false;
            }
            if let Some(fields) = &mut node_shape_json.fields {
                for field in fields.values_mut() {
                    field.required = false;
                }
            }
        }
        VariableType::Anonymous => anonymous_node_shapes.push(NodeShapeJSON {
            kind: name.to_string(),
            named: false,
            root: false,
            extra: extra_names.contains(name),
            fields: None,
            children: None,
            subtypes: None,
        }),
        _ => {}
    }
}

fn symbol_node_metadata<'a>(
    symbol: Symbol,
    syntax_grammar: &'a SyntaxGrammar,
    lexical_grammar: &'a LexicalGrammar,
) -> (&'a str, VariableType) {
    match symbol.kind {
        SymbolType::NonTerminal => {
            let variable = &syntax_grammar.variables[symbol.index];
            (&variable.name, variable.kind)
        }
        SymbolType::Terminal => {
            let variable = &lexical_grammar.variables[symbol.index];
            (&variable.name, variable.kind)
        }
        SymbolType::External => {
            let token = &syntax_grammar.external_tokens[symbol.index];
            (&token.name, token.kind)
        }
        SymbolType::End | SymbolType::EndOfNonTerminalExtra => panic!("Unexpected symbol type"),
    }
}

fn sort_subtypes_topologically(
    subtype_map: &mut [(NodeTypeJSON, Vec<NodeTypeJSON>)],
) -> SuperTypeCycleResult<()> {
    let mut sorted_kinds = Vec::with_capacity(subtype_map.len());
    let mut top_sort = topological_sort::TopologicalSort::<String>::new();
    for (supertype, subtypes) in subtype_map.iter() {
        for subtype in subtypes {
            top_sort.add_dependency(subtype.kind.clone(), supertype.kind.clone());
        }
    }

    loop {
        let mut next_kinds = top_sort.pop_all();
        match (next_kinds.is_empty(), top_sort.is_empty()) {
            (true, true) => break,
            (true, false) => {
                let mut items = top_sort.collect::<Vec<String>>();
                items.sort();
                return Err(SuperTypeCycleError { items });
            }
            (false, _) => {
                next_kinds.sort();
                sorted_kinds.extend(next_kinds);
            }
        }
    }

    subtype_map.sort_by(|a, b| {
        let a_idx = sorted_kinds.iter().position(|n| n.eq(&a.0.kind)).unwrap();
        let b_idx = sorted_kinds.iter().position(|n| n.eq(&b.0.kind)).unwrap();
        a_idx.cmp(&b_idx)
    });
    Ok(())
}

fn process_supertypes(info: &mut FieldInfoJSON, subtype_map: &[(NodeTypeJSON, Vec<NodeTypeJSON>)]) {
    for (supertype, subtypes) in subtype_map {
        if info.types.contains(supertype) {
            info.types.retain(|t| !subtypes.contains(t));
        }
    }
}

fn variable_type_for_child_type(
    child_type: &ChildType,
    syntax_grammar: &SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
) -> VariableType {
    match child_type {
        ChildType::Aliased(alias) => alias.kind(),
        ChildType::Normal(symbol) => {
            if syntax_grammar.supertype_symbols.contains(symbol) {
                VariableType::Named
            } else if syntax_grammar.variables_to_inline.contains(symbol)
                || matches!(
                    symbol.kind,
                    SymbolType::End | SymbolType::EndOfNonTerminalExtra
                )
            {
                VariableType::Hidden
            } else {
                symbol_node_metadata(*symbol, syntax_grammar, lexical_grammar).1
            }
        }
    }
}

fn extend_sorted<'a, T>(vec: &mut Vec<T>, values: impl IntoIterator<Item = &'a T>) -> bool
where
    T: 'a + Clone + Eq + Ord,
{
    values.into_iter().fold(false, |acc, value| {
        if let Err(i) = vec.binary_search(value) {
            vec.insert(i, value.clone());
            true
        } else {
            acc
        }
    })
}
