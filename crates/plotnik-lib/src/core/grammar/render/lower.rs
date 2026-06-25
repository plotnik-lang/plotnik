//! Lowering the grammar pipeline into a [`TreeGrammar`].
//!
//! The renderer taps the pipeline **after `extract_tokens`, before
//! `expand_repeats`**: nested `Seq`/`Choice`/`Repeat` structure is intact, symbols
//! are resolved, and tokens are split out. Lowering happens in two passes so the
//! grammars never need to be cloned:
//!
//! 1. [`lower`] reads the still-owned extracted grammars in place and produces the
//!    pattern/token/external definitions and the extras list (no closures yet).
//! 2. [`attach_node_shapes`] joins in the visible subtype closures (and the
//!    `root`/`extra` flags) once `node_shapes` has been computed downstream.
//!
//! Lowering `Rule → Shape` is the one place every string-language construct
//! (precedence, reserved contexts, token metadata, blanks) is normalized away.

use std::collections::HashSet;

use super::super::prepared::{ExtractedLexicalGrammar, ExtractedSyntaxGrammar, VariableType};
use super::super::rules::{Rule, Symbol, SymbolType};
use super::super::types::{NodeKindRef, NodeShape};
use super::lexical::synthesize;
use super::{Body, Def, DefKind, NodeRef, Quant, Shape, TreeGrammar};

/// A kind that exists only by aliasing another: `(alias_name)` displays the
/// `underlying` rule under a new name.
pub(in crate::core::grammar) struct AliasInfo {
    name: String,
    named: bool,
    underlying: String,
    /// The underlying rule's reference shape, used when it has no own definition
    /// to borrow a body from (e.g. an aliased anonymous token).
    underlying_shape: Shape,
}

/// Pass 1: build the tree-shape model's definitions and extras from the tapped
/// grammars. Categories are emitted with empty member lists; `root`/`extra` are
/// left unset until [`attach_node_shapes`]. The collected alias usages are
/// returned for the same pass to turn into alias-only definitions.
pub(in crate::core::grammar) fn lower(
    name: String,
    order: &[String],
    syntax: &ExtractedSyntaxGrammar,
    lexical: &ExtractedLexicalGrammar,
) -> (TreeGrammar, Vec<AliasInfo>) {
    let ctx = LowerCtx::new(syntax, lexical);

    let mut defs = Vec::new();
    for rule_name in order {
        if let Some(def) = ctx.lower_def(rule_name) {
            defs.push(def);
        }
    }
    for token in &syntax.external_tokens {
        // Internal-token-backed externals render through their lexical token, not here.
        if token.corresponding_internal_token.is_none() {
            defs.push(Def {
                name: token.name.clone(),
                kind: DefKind::External,
                extra: false,
                root: false,
                body: Body::None,
            });
        }
    }

    let tree = TreeGrammar {
        name,
        defs,
        extras: ctx.lower_extras(),
        root: None,
    };
    (tree, ctx.collect_aliases())
}

/// Pass 2: fill category member lists from the visible subtype closures, set the
/// `root`/`extra` annotation flags, and append alias-only kinds as definitions.
pub(in crate::core::grammar) fn attach_node_shapes(
    tree: &mut TreeGrammar,
    node_shapes: &[NodeShape],
    aliases: &[AliasInfo],
) {
    let supertype_names: HashSet<&str> = node_shapes
        .iter()
        .filter(|shape| shape.subtypes.is_some())
        .map(|shape| shape.kind_name.as_str())
        .collect();
    let extra_names: HashSet<&str> = node_shapes
        .iter()
        .filter(|shape| shape.extra)
        .map(|shape| shape.kind_name.as_str())
        .collect();
    let root_name = node_shapes
        .iter()
        .find(|shape| shape.root)
        .map(|shape| shape.kind_name.as_str());

    tree.root = root_name.map(str::to_string);

    for def in &mut tree.defs {
        def.extra = extra_names.contains(def.name.as_str());
        def.root = root_name == Some(def.name.as_str());

        if let Body::Category(members) = &mut def.body {
            for shape in node_shapes {
                let Some(subtypes) = &shape.subtypes else {
                    continue;
                };
                if de_underscore(&shape.kind_name) == def.name {
                    *members = closure_members(subtypes, &supertype_names);
                    break;
                }
            }
        }
    }

    append_alias_defs(tree, node_shapes, aliases, &extra_names);
}

/// Reconstruct alias-only kinds (e.g. `property_identifier`, an alias of
/// `identifier`) as definitions, so the dump's discovery surface is complete. A
/// kind already produced directly keeps its own definition; this only adds the
/// ones that exist *solely* as aliases.
fn append_alias_defs(
    tree: &mut TreeGrammar,
    node_shapes: &[NodeShape],
    aliases: &[AliasInfo],
    extra_names: &HashSet<&str>,
) {
    use std::collections::BTreeMap;

    let defined: HashSet<&str> = tree.defs.iter().map(|def| def.name.as_str()).collect();
    let surfaced: HashSet<&str> = node_shapes
        .iter()
        .filter(|shape| shape.named)
        .map(|shape| shape.kind_name.as_str())
        .collect();

    // Sorted + deduplicated so output is deterministic and each alias is defined once.
    let mut pending: BTreeMap<&str, &AliasInfo> = BTreeMap::new();
    for alias in aliases {
        if alias.named
            && !defined.contains(alias.name.as_str())
            && surfaced.contains(alias.name.as_str())
        {
            pending.entry(alias.name.as_str()).or_insert(alias);
        }
    }

    let new_defs: Vec<Def> = pending
        .values()
        .map(|alias| {
            let body = tree
                .defs
                .iter()
                .find(|def| def.name == alias.underlying)
                .map(|def| def.body.clone())
                .unwrap_or_else(|| Body::Pattern(alias.underlying_shape.clone()));
            Def {
                name: alias.name.clone(),
                kind: DefKind::AliasOf(alias.underlying.clone()),
                extra: extra_names.contains(alias.name.as_str()),
                root: false,
                body,
            }
        })
        .collect();

    tree.defs.extend(new_defs);
}

fn closure_members(refs: &[NodeKindRef], supertype_names: &HashSet<&str>) -> Vec<NodeRef> {
    refs.iter()
        .map(|kind_ref| {
            if supertype_names.contains(kind_ref.kind_name.as_str()) {
                NodeRef {
                    name: de_underscore(&kind_ref.kind_name).to_string(),
                    named: true,
                    category: true,
                }
            } else {
                NodeRef {
                    name: kind_ref.kind_name.clone(),
                    named: kind_ref.named,
                    category: false,
                }
            }
        })
        .collect()
}

struct LowerCtx<'a> {
    syntax: &'a ExtractedSyntaxGrammar,
    lexical: &'a ExtractedLexicalGrammar,
    supertypes: HashSet<Symbol>,
    inlines: HashSet<Symbol>,
    supertype_names: HashSet<&'a str>,
}

impl<'a> LowerCtx<'a> {
    fn new(syntax: &'a ExtractedSyntaxGrammar, lexical: &'a ExtractedLexicalGrammar) -> Self {
        let supertypes: HashSet<Symbol> = syntax.supertype_symbols.iter().copied().collect();
        let inlines: HashSet<Symbol> = syntax.variables_to_inline.iter().copied().collect();
        let supertype_names = supertypes
            .iter()
            .filter_map(|symbol| match symbol.kind {
                SymbolType::NonTerminal => Some(syntax.variables[symbol.index].name.as_str()),
                _ => None,
            })
            .collect();

        Self {
            syntax,
            lexical,
            supertypes,
            inlines,
            supertype_names,
        }
    }

    /// Build a definition for one declared rule name, or `None` if it was dropped
    /// as unreachable (and so is absent from both grammars).
    fn lower_def(&self, rule_name: &str) -> Option<Def> {
        // Categories first: a supertype renders from its closure (attached later),
        // not from its rule body.
        if self.supertype_names.contains(rule_name) {
            return Some(Def {
                name: de_underscore(rule_name).to_string(),
                kind: DefKind::Category,
                extra: false,
                root: false,
                body: Body::Category(Vec::new()),
            });
        }

        if let Some((index, variable)) = self.find(&self.syntax.variables, rule_name) {
            let symbol = Symbol::non_terminal(index);
            let hidden = variable.kind == VariableType::Hidden || self.inlines.contains(&symbol);
            return Some(Def {
                name: rule_name.to_string(),
                kind: if hidden { DefKind::Hidden } else { DefKind::Node },
                extra: false,
                root: false,
                body: Body::Pattern(self.lower_shape(&variable.rule)),
            });
        }

        if let Some((_, variable)) = self.find(&self.lexical.variables, rule_name) {
            return Some(Def {
                name: rule_name.to_string(),
                kind: DefKind::Token,
                extra: false,
                root: false,
                body: Body::Token(synthesize(&variable.rule)),
            });
        }

        None
    }

    fn find<'v>(
        &self,
        variables: &'v [super::super::prepared::Variable],
        rule_name: &str,
    ) -> Option<(usize, &'v super::super::prepared::Variable)> {
        variables
            .iter()
            .enumerate()
            .find(|(_, variable)| variable.name == rule_name)
    }

    /// Find every alias that renames a single symbol, so alias-only kinds can be
    /// reconstructed as their own definitions.
    fn collect_aliases(&self) -> Vec<AliasInfo> {
        let mut aliases = Vec::new();
        for variable in &self.syntax.variables {
            self.walk_aliases(&variable.rule, &mut aliases);
        }
        aliases
    }

    fn walk_aliases(&self, rule: &Rule, out: &mut Vec<AliasInfo>) {
        match rule {
            Rule::Metadata { params, rule } => {
                if let Some(alias) = &params.alias
                    && let Some((underlying, underlying_shape)) = self.single_symbol(rule)
                {
                    out.push(AliasInfo {
                        name: alias.value.clone(),
                        named: alias.is_named,
                        underlying,
                        underlying_shape,
                    });
                }
                self.walk_aliases(rule, out);
            }
            Rule::Seq(members) | Rule::Choice(members) => {
                for member in members {
                    self.walk_aliases(member, out);
                }
            }
            Rule::Repeat(inner) | Rule::Reserved { rule: inner, .. } => {
                self.walk_aliases(inner, out);
            }
            _ => {}
        }
    }

    /// The public name and reference shape of the single symbol a rule reduces
    /// to, if any.
    fn single_symbol(&self, rule: &Rule) -> Option<(String, Shape)> {
        let Rule::Symbol(symbol) = peel(rule) else {
            return None;
        };
        let name = match symbol.kind {
            SymbolType::NonTerminal => self.syntax.variables[symbol.index].name.clone(),
            SymbolType::Terminal => self.lexical.variables[symbol.index].name.clone(),
            SymbolType::External => self.syntax.external_tokens[symbol.index].name.clone(),
            _ => return None,
        };
        Some((name, self.resolve_symbol(*symbol)))
    }

    fn lower_extras(&self) -> Vec<Shape> {
        let mut shapes = Vec::new();
        for symbol in &self.syntax.extra_symbols {
            shapes.push(self.resolve_symbol(*symbol));
        }
        for separator in &self.lexical.separators {
            shapes.push(Shape::Token(synthesize(separator)));
        }
        shapes
    }

    fn lower_shape(&self, rule: &Rule) -> Shape {
        match rule {
            Rule::Blank => Shape::Empty,
            Rule::Symbol(symbol) => self.resolve_symbol(*symbol),
            Rule::Seq(members) => self.lower_seq(members),
            Rule::Choice(members) => self.lower_choice(members),
            Rule::Repeat(inner) => {
                Shape::Quantified(Box::new(self.lower_shape(inner)), Quant::Plus)
            }
            Rule::Metadata { params, rule } => {
                let inner = if let Some(alias) = &params.alias {
                    ref_shape(alias.value.clone(), alias.is_named)
                } else {
                    self.lower_shape(rule)
                };
                match &params.field_name {
                    Some(field) => Shape::Field(field.clone(), Box::new(inner)),
                    None => inner,
                }
            }
            Rule::Reserved { rule, .. } => self.lower_shape(rule),
            // No String/Pattern/NamedSymbol survive into a syntax rule after
            // `extract_tokens`; handled defensively so lowering is total.
            Rule::String(value) => Shape::Node(NodeRef {
                name: value.clone(),
                named: false,
                category: false,
            }),
            Rule::Pattern(value, flags) => {
                Shape::Token(synthesize(&Rule::Pattern(value.clone(), flags.clone())))
            }
            Rule::NamedSymbol(name) => Shape::Splice(name.clone()),
        }
    }

    fn lower_seq(&self, members: &[Rule]) -> Shape {
        let mut shapes: Vec<Shape> = Vec::with_capacity(members.len());
        for member in members {
            match self.lower_shape(member) {
                Shape::Empty => {}
                // A bare sequence directly inside a sequence adds no grouping —
                // concatenation is associative, so splice its members in. (Only
                // an unquantified `Seq` matches; `X*` is `Quantified(Seq, …)`.)
                Shape::Seq(inner) => shapes.extend(inner),
                other => shapes.push(other),
            }
        }
        collapse(shapes, Shape::Seq)
    }

    fn lower_choice(&self, members: &[Rule]) -> Shape {
        let has_blank = members.iter().any(is_blank);
        let non_blank: Vec<&Rule> = members.iter().filter(|m| !is_blank(m)).collect();

        // `choice(repeat(X), blank)` is tree-sitter's encoding of `X*`.
        if has_blank
            && let [single] = non_blank.as_slice()
            && let Rule::Repeat(inner) = peel(single)
        {
            return Shape::Quantified(Box::new(self.lower_shape(inner)), Quant::Star);
        }

        let inner = {
            let shapes: Vec<Shape> = non_blank
                .iter()
                .map(|member| self.lower_shape(member))
                .filter(|shape| !matches!(shape, Shape::Empty))
                .collect();
            collapse(shapes, Shape::Choice)
        };

        if has_blank {
            Shape::Quantified(Box::new(inner), Quant::Optional)
        } else {
            inner
        }
    }

    fn resolve_symbol(&self, symbol: Symbol) -> Shape {
        match symbol.kind {
            SymbolType::NonTerminal => {
                let variable = &self.syntax.variables[symbol.index];
                if self.supertypes.contains(&symbol) {
                    Shape::Node(NodeRef {
                        name: de_underscore(&variable.name).to_string(),
                        named: true,
                        category: true,
                    })
                } else if variable.kind == VariableType::Hidden || self.inlines.contains(&symbol) {
                    Shape::Splice(variable.name.clone())
                } else {
                    Shape::Node(NodeRef {
                        name: variable.name.clone(),
                        named: true,
                        category: false,
                    })
                }
            }
            SymbolType::Terminal => {
                let variable = &self.lexical.variables[symbol.index];
                match variable.kind {
                    VariableType::Named => Shape::Node(NodeRef {
                        name: variable.name.clone(),
                        named: true,
                        category: false,
                    }),
                    VariableType::Anonymous => Shape::Node(NodeRef {
                        name: variable.name.clone(),
                        named: false,
                        category: false,
                    }),
                    // An auxiliary/hidden regex token has no queryable name; splice
                    // its synthesized text inline.
                    VariableType::Auxiliary | VariableType::Hidden => {
                        Shape::Token(synthesize(&variable.rule))
                    }
                }
            }
            SymbolType::External => {
                let token = &self.syntax.external_tokens[symbol.index];
                match token.kind {
                    VariableType::Anonymous => Shape::Node(NodeRef {
                        name: token.name.clone(),
                        named: false,
                        category: false,
                    }),
                    // A hidden external (underscore-prefixed) is consumed but never
                    // a node a query can match, so it splices like any hidden rule.
                    VariableType::Hidden | VariableType::Auxiliary => {
                        Shape::Splice(token.name.clone())
                    }
                    VariableType::Named => Shape::Node(NodeRef {
                        name: token.name.clone(),
                        named: true,
                        category: false,
                    }),
                }
            }
            SymbolType::End | SymbolType::EndOfNonTerminalExtra => Shape::Empty,
        }
    }
}

/// Collapse a single-member group to its member and an empty group to `Empty`.
fn collapse(mut shapes: Vec<Shape>, wrap: impl FnOnce(Vec<Shape>) -> Shape) -> Shape {
    match shapes.len() {
        0 => Shape::Empty,
        1 => shapes.pop().expect("one shape"),
        _ => wrap(shapes),
    }
}

/// Peel transparent precedence/reserved wrappers (but keep field/alias metadata,
/// which changes the shape).
fn peel(rule: &Rule) -> &Rule {
    match rule {
        Rule::Reserved { rule, .. } => peel(rule),
        Rule::Metadata { params, rule }
            if params.field_name.is_none() && params.alias.is_none() =>
        {
            peel(rule)
        }
        other => other,
    }
}

fn is_blank(rule: &Rule) -> bool {
    matches!(peel(rule), Rule::Blank)
}

/// A named reference whose name starts with `_` is a hidden rule the query
/// language cannot write (`_` is the wildcard), so it splices rather than render
/// as the un-queryable `(_name)`. Anonymous tokens keep their literal text.
fn ref_shape(name: String, named: bool) -> Shape {
    if named && name.starts_with('_') {
        Shape::Splice(name)
    } else {
        Shape::Node(NodeRef {
            name,
            named,
            category: false,
        })
    }
}

/// Strip the leading underscores tree-sitter uses to mark hidden categories.
fn de_underscore(name: &str) -> &str {
    let trimmed = name.trim_start_matches('_');
    if trimmed.is_empty() { name } else { trimmed }
}
