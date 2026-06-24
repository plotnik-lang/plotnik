//! Raw grammar lowering helpers.

use std::collections::BTreeSet;

use rustc_hash::{FxHashMap, FxHashSet};

use super::raw::{RawGrammar, RawPrecedence, RawPrecedenceEntry, RawRule};
use super::{
    node_shapes::{self, GrammarContext},
    prepared::{
        InlinedProductionMap, LexicalGrammar, PrecedenceEntry, Production, ReservedWordSet,
        SyntaxGrammar, Variable, VariableType,
    },
    rules::{Alias, AliasMap, Precedence, Rule, Symbol, SymbolType},
    types::{FieldEntry, NodeKindEntry},
};

const TREE_SITTER_PUBLIC_NAME_SEPARATOR: char = '\0';
const FIRST_SYMBOL_POSITION_AFTER_END: usize = 1;

pub(super) struct LoweredGrammar {
    pub variables: Vec<Variable>,
    pub extra_symbols: Vec<Rule>,
    pub expected_conflicts: Vec<Vec<String>>,
    pub precedence_orderings: Vec<Vec<PrecedenceEntry>>,
    pub external_tokens: Vec<Rule>,
    pub variables_to_inline: Vec<String>,
    pub supertype_symbols: Vec<String>,
    pub word_token: Option<String>,
    pub reserved_words: Vec<ReservedWordSet<Rule>>,
}

#[derive(Clone, Copy)]
pub(super) struct UninternedGrammar<'a> {
    pub variables: &'a [Variable],
    pub extra_symbols: &'a [Rule],
    pub expected_conflicts: &'a [Vec<String>],
    pub external_tokens: &'a [Rule],
    pub variables_to_inline: &'a [String],
    pub supertype_symbols: &'a [String],
    pub word_token: Option<&'a str>,
    pub reserved_words: &'a [ReservedWordSet<Rule>],
}

impl LoweredGrammar {
    pub fn from_raw(raw: &RawGrammar) -> Self {
        Self {
            variables: raw
                .rules
                .iter()
                .map(|(name, rule)| Variable {
                    name: name.clone(),
                    kind: VariableType::Named,
                    rule: convert_rule(rule),
                })
                .collect(),
            extra_symbols: raw.extras.iter().map(convert_rule).collect(),
            expected_conflicts: raw.conflicts.clone(),
            precedence_orderings: raw
                .precedences
                .iter()
                .map(|entries| entries.iter().map(convert_precedence_entry).collect())
                .collect(),
            external_tokens: raw.externals.iter().map(convert_rule).collect(),
            variables_to_inline: raw.inline.clone(),
            supertype_symbols: raw.supertypes.clone(),
            word_token: raw.word.clone(),
            reserved_words: raw
                .reserved
                .iter()
                .map(|(name, rules)| ReservedWordSet {
                    name: name.clone(),
                    reserved_words: rules.iter().map(convert_rule).collect(),
                })
                .collect(),
        }
    }

    pub fn as_uninterned(&self) -> UninternedGrammar<'_> {
        UninternedGrammar {
            variables: &self.variables,
            extra_symbols: &self.extra_symbols,
            expected_conflicts: &self.expected_conflicts,
            external_tokens: &self.external_tokens,
            variables_to_inline: &self.variables_to_inline,
            supertype_symbols: &self.supertype_symbols,
            word_token: self.word_token.as_deref(),
            reserved_words: &self.reserved_words,
        }
    }

    pub fn retain_reachable_rules(&mut self) {
        let used = reachable_rule_names(
            &self.variables,
            self.word_token.as_deref(),
            &self.extra_symbols,
            &self.external_tokens,
            &self.reserved_words,
        );
        let dropped = self
            .variables
            .iter()
            .filter(|variable| !used.contains(variable.name.as_str()))
            .map(|variable| variable.name.clone())
            .collect::<Vec<_>>();

        self.variables
            .retain(|variable| used.contains(variable.name.as_str()));

        let dropped: FxHashSet<&str> = dropped.iter().map(String::as_str).collect();
        self.remove_dropped_rule_references(&dropped);
    }

    fn remove_dropped_rule_references(&mut self, dropped: &FxHashSet<&str>) {
        self.expected_conflicts.retain(|conflict| {
            !conflict
                .iter()
                .any(|symbol| dropped.contains(symbol.as_str()))
        });
        self.supertype_symbols
            .retain(|symbol| !dropped.contains(symbol.as_str()));
        self.variables_to_inline
            .retain(|symbol| !dropped.contains(symbol.as_str()));
        self.extra_symbols
            .retain(|rule| !rule_references_any(rule, dropped, true));
        self.external_tokens
            .retain(|rule| !rule_references_any(rule, dropped, true));
        self.precedence_orderings.retain(|ordering| {
            !ordering.iter().any(|entry| {
                matches!(entry, PrecedenceEntry::Symbol(symbol) if dropped.contains(symbol.as_str()))
            })
        });
        for context in &mut self.reserved_words {
            context
                .reserved_words
                .retain(|rule| !rule_references_any(rule, dropped, false));
        }
    }
}

fn reachable_rule_names<'a>(
    variables: &'a [Variable],
    word_token: Option<&'a str>,
    extra_symbols: &'a [Rule],
    external_tokens: &'a [Rule],
    reserved_words: &'a [ReservedWordSet<Rule>],
) -> FxHashSet<String> {
    let by_name = variables
        .iter()
        .map(|variable| (variable.name.as_str(), &variable.rule))
        .collect::<FxHashMap<_, _>>();
    let mut visited = FxHashSet::<&str>::default();
    let mut stack = Vec::<&str>::new();

    if let Some(start) = variables.first() {
        stack.push(start.name.as_str());
    }
    if let Some(word_token) = word_token {
        stack.push(word_token);
    }
    for rule in extra_symbols {
        collect_referenced_names(rule, false, &mut stack);
    }
    for rule in external_tokens {
        collect_referenced_names(rule, true, &mut stack);
    }
    for context in reserved_words {
        for rule in &context.reserved_words {
            collect_referenced_names(rule, false, &mut stack);
        }
    }

    while let Some(name) = stack.pop() {
        if !visited.insert(name) {
            continue;
        }
        if let Some(rule) = by_name.get(name) {
            collect_referenced_names(rule, false, &mut stack);
        }
    }

    visited.into_iter().map(String::from).collect()
}

fn collect_referenced_names<'a>(rule: &'a Rule, skip_top_level: bool, out: &mut Vec<&'a str>) {
    for_each_referenced_name(rule, skip_top_level, &mut |name| out.push(name));
}

fn rule_references_any(rule: &Rule, targets: &FxHashSet<&str>, skip_top_level: bool) -> bool {
    let mut found = false;
    for_each_referenced_name(rule, skip_top_level, &mut |name| {
        found |= targets.contains(name);
    });
    found
}

fn for_each_referenced_name<'a, F>(rule: &'a Rule, skip_top_level: bool, visit: &mut F)
where
    F: FnMut(&'a str),
{
    match rule {
        Rule::NamedSymbol(name) => {
            if !skip_top_level {
                visit(name.as_str());
            }
        }
        Rule::Choice(rules) | Rule::Seq(rules) => {
            for rule in rules {
                for_each_referenced_name(rule, false, visit);
            }
        }
        Rule::Metadata { rule, .. } | Rule::Reserved { rule, .. } => {
            for_each_referenced_name(rule, skip_top_level, visit);
        }
        Rule::Repeat(rule) => for_each_referenced_name(rule, false, visit),
        Rule::Blank | Rule::String(_) | Rule::Pattern(_, _) | Rule::Symbol(_) => {}
    }
}

pub(super) fn convert_rule(rule: &RawRule) -> Rule {
    match rule {
        RawRule::BLANK => Rule::Blank,
        RawRule::STRING { value } => Rule::String(value.clone()),
        RawRule::PATTERN { value, flags } => Rule::Pattern(
            value.clone(),
            flags.as_deref().map(filter_flags).unwrap_or_default(),
        ),
        RawRule::SYMBOL { name } => Rule::NamedSymbol(name.clone()),
        RawRule::SEQ { members } => Rule::seq(members.iter().map(convert_rule).collect()),
        RawRule::CHOICE { members } => Rule::choice(members.iter().map(convert_rule).collect()),
        RawRule::REPEAT { content } => {
            Rule::choice(vec![Rule::repeat(convert_rule(content)), Rule::Blank])
        }
        RawRule::REPEAT1 { content } => Rule::repeat(convert_rule(content)),
        RawRule::FIELD { name, content } => Rule::field(name.clone(), convert_rule(content)),
        RawRule::ALIAS {
            content,
            value,
            named,
        } => Rule::alias(convert_rule(content), value.clone(), *named),
        RawRule::TOKEN { content } => Rule::token(convert_rule(content)),
        RawRule::IMMEDIATE_TOKEN { content } => Rule::immediate_token(convert_rule(content)),
        RawRule::PREC { value, content } => {
            Rule::prec(convert_precedence(value), convert_rule(content))
        }
        RawRule::PREC_LEFT { value, content } => {
            Rule::prec_left(convert_precedence(value), convert_rule(content))
        }
        RawRule::PREC_RIGHT { value, content } => {
            Rule::prec_right(convert_precedence(value), convert_rule(content))
        }
        RawRule::PREC_DYNAMIC { value, content } => {
            Rule::prec_dynamic(*value, convert_rule(content))
        }
        RawRule::RESERVED {
            context_name,
            content,
        } => Rule::Reserved {
            rule: Box::new(convert_rule(content)),
            context_name: context_name.clone(),
        },
    }
}

fn filter_flags(flags: &str) -> String {
    // Tree-sitter's regex lowering only preserves case-insensitive matching here.
    flags.chars().filter(|flag| *flag == 'i').collect()
}

fn convert_precedence(precedence: &RawPrecedence) -> Precedence {
    match precedence {
        RawPrecedence::Integer(value) => Precedence::Integer(*value),
        RawPrecedence::Name(name) => Precedence::Name(name.clone()),
    }
}

pub(super) fn convert_precedence_entry(entry: &RawPrecedenceEntry) -> PrecedenceEntry {
    match entry {
        RawPrecedenceEntry::STRING { value } => PrecedenceEntry::Name(value.clone()),
        RawPrecedenceEntry::SYMBOL { name } => PrecedenceEntry::Symbol(name.clone()),
    }
}

pub(super) fn derive_fields(
    syntax_grammar: &SyntaxGrammar,
    inlines: &InlinedProductionMap,
    variable_info: &[node_shapes::VariableSummary],
) -> Vec<FieldEntry> {
    let mut field_names = BTreeSet::<String>::new();
    for_each_metadata_production(syntax_grammar, inlines, |production| {
        collect_field_names(production, syntax_grammar, variable_info, &mut field_names);
    });

    field_names
        .into_iter()
        .enumerate()
        .map(|(index, name)| FieldEntry {
            id: u16::try_from(index + 1).expect("tree-sitter field IDs fit in u16"),
            name,
        })
        .collect()
}

fn collect_field_names(
    production: &Production,
    syntax_grammar: &SyntaxGrammar,
    variable_info: &[node_shapes::VariableSummary],
    field_names: &mut BTreeSet<String>,
) {
    for step in &production.steps {
        if let Some(field_name) = &step.field_name {
            field_names.insert(field_name.clone());
        }

        if step.symbol.is_non_terminal()
            && !syntax_grammar.variables[step.symbol.index]
                .kind
                .is_visible()
        {
            field_names.extend(variable_info[step.symbol.index].fields.keys().cloned());
        }
    }
}

pub(super) fn derive_symbols(
    syntax_grammar: &SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
    inlines: &InlinedProductionMap,
    default_aliases: &AliasMap,
) -> Vec<NodeKindEntry> {
    let ctx = GrammarContext {
        syntax: syntax_grammar,
        lexical: lexical_grammar,
        aliases: default_aliases,
    };
    let symbol_order = derive_symbol_order(syntax_grammar, lexical_grammar);
    let symbol_ids = symbol_ids(&symbol_order);
    let symbol_map = public_symbol_map(
        syntax_grammar,
        lexical_grammar,
        &symbol_order,
        default_aliases,
    );
    let resolution = SymbolResolution {
        ids: &symbol_ids,
        map: &symbol_map,
    };
    let unique_aliases = unique_aliases(ctx, inlines, &symbol_order, resolution);

    let mut symbols = Vec::new();
    for symbol in &symbol_order {
        let public_id = symbol_ids[&symbol_map[symbol]];
        let (type_name, kind) = default_aliases.get(symbol).map_or_else(
            || metadata_for_symbol(syntax_grammar, lexical_grammar, *symbol),
            |alias| (alias.value.as_str(), alias.kind()),
        );
        let visibility = symbol_visibility(syntax_grammar, *symbol, kind, default_aliases);

        if public_id == 0 {
            continue;
        }

        symbols.push(NodeKindEntry {
            id: public_id,
            type_name: public_node_kind(type_name),
            named: visibility.named,
            visible: visibility.visible,
            supertype: visibility.supertype,
            terminal: symbol.is_terminal() || symbol.is_external(),
        });
    }

    let first_alias_id = symbol_ids
        .values()
        .copied()
        .max()
        .expect("tree-sitter symbol order includes end symbol")
        + 1;
    for (index, alias) in unique_aliases.iter().enumerate() {
        symbols.push(NodeKindEntry::alias(
            first_alias_id + u16::try_from(index).expect("tree-sitter alias IDs fit in u16"),
            public_node_kind(&alias.value),
            alias.is_named,
        ));
    }

    symbols
}

pub(super) fn public_node_kind(name: &str) -> String {
    // Tree-sitter appends private disambiguators after NUL; public node names stop before it.
    name.split(TREE_SITTER_PUBLIC_NAME_SEPARATOR)
        .next()
        .unwrap_or(name)
        .to_string()
}

fn derive_symbol_order(
    syntax_grammar: &SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
) -> Vec<Symbol> {
    let mut symbols = Vec::with_capacity(
        1 + lexical_grammar.variables.len()
            + syntax_grammar.external_tokens.len()
            + syntax_grammar.variables.len(),
    );

    symbols.push(Symbol::end());

    for index in 0..lexical_grammar.variables.len() {
        let symbol = Symbol::terminal(index);
        if syntax_grammar.word_token == Some(symbol) {
            symbols.insert(FIRST_SYMBOL_POSITION_AFTER_END, symbol);
        } else {
            symbols.push(symbol);
        }
    }

    for (index, token) in syntax_grammar.external_tokens.iter().enumerate() {
        if token.corresponding_internal_token.is_none() {
            symbols.push(Symbol::external(index));
        }
    }

    for index in 0..syntax_grammar.variables.len() {
        let symbol = Symbol::non_terminal(index);
        if !syntax_grammar.variables_to_inline.contains(&symbol) {
            symbols.push(symbol);
        }
    }

    symbols
}

fn symbol_ids(symbols: &[Symbol]) -> rustc_hash::FxHashMap<Symbol, u16> {
    let mut ids = rustc_hash::FxHashMap::default();
    ids.insert(Symbol::end(), 0);

    let mut next_id = 1u16;
    for symbol in symbols {
        if *symbol == Symbol::end() {
            continue;
        }
        ids.insert(*symbol, next_id);
        next_id = next_id
            .checked_add(1)
            .expect("tree-sitter symbol IDs fit in u16");
    }

    ids.insert(Symbol::end_of_nonterminal_extra(), 0);
    ids
}

fn public_symbol_map(
    syntax_grammar: &SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
    symbols: &[Symbol],
    default_aliases: &AliasMap,
) -> rustc_hash::FxHashMap<Symbol, Symbol> {
    let mut symbol_map = rustc_hash::FxHashMap::default();

    for symbol in symbols {
        let mut mapping = *symbol;

        if let Some(alias) = default_aliases.get(symbol) {
            let kind = alias.kind();
            for other_symbol in symbols {
                if let Some(other_alias) = default_aliases.get(other_symbol) {
                    if other_symbol < &mapping && other_alias == alias {
                        mapping = *other_symbol;
                    }
                } else if metadata_for_symbol(syntax_grammar, lexical_grammar, *other_symbol)
                    == (alias.value.as_str(), kind)
                {
                    mapping = *other_symbol;
                    break;
                }
            }
        } else if symbol.is_terminal() {
            let metadata = metadata_for_symbol(syntax_grammar, lexical_grammar, *symbol);
            for other_symbol in symbols {
                if metadata_for_symbol(syntax_grammar, lexical_grammar, *other_symbol) == metadata {
                    if let Some(mapped) = symbol_map.get(other_symbol)
                        && mapped == symbol
                    {
                        break;
                    }
                    mapping = *other_symbol;
                    break;
                }
            }
        }

        symbol_map.insert(*symbol, mapping);
    }

    symbol_map
}

/// The public symbol id assignment paired with the canonical-symbol map that
/// `unique_aliases` consults to decide whether an alias already names an existing symbol.
#[derive(Clone, Copy)]
struct SymbolResolution<'a> {
    ids: &'a rustc_hash::FxHashMap<Symbol, u16>,
    map: &'a rustc_hash::FxHashMap<Symbol, Symbol>,
}

fn unique_aliases(
    ctx: GrammarContext<'_>,
    inlines: &InlinedProductionMap,
    symbols: &[Symbol],
    resolution: SymbolResolution<'_>,
) -> Vec<Alias> {
    let mut aliases = Vec::new();

    for_each_metadata_production(ctx.syntax, inlines, |production| {
        for alias in production
            .steps
            .iter()
            .filter_map(|step| step.alias.as_ref())
        {
            let has_existing_symbol = symbol_for_alias(ctx, symbols, alias)
                .and_then(|symbol| resolution.map.get(&symbol))
                .is_some_and(|symbol| resolution.ids.contains_key(symbol));

            if has_existing_symbol {
                continue;
            }

            if let Err(index) = aliases.binary_search(alias) {
                aliases.insert(index, alias.clone());
            }
        }
    });

    aliases
}

fn for_each_metadata_production(
    syntax_grammar: &SyntaxGrammar,
    inlines: &InlinedProductionMap,
    mut visit: impl FnMut(&Production),
) {
    for variable in &syntax_grammar.variables {
        for production in &variable.productions {
            visit(production);
        }
    }

    for production in &inlines.productions {
        visit(production);
    }
}

fn symbol_for_alias(ctx: GrammarContext<'_>, symbols: &[Symbol], alias: &Alias) -> Option<Symbol> {
    symbols.iter().copied().find(|symbol| {
        ctx.aliases.get(symbol).map_or_else(
            || {
                let (name, kind) = metadata_for_symbol(ctx.syntax, ctx.lexical, *symbol);
                name == alias.value && kind == alias.kind()
            },
            |default_alias| default_alias == alias,
        )
    })
}

fn metadata_for_symbol<'a>(
    syntax_grammar: &'a SyntaxGrammar,
    lexical_grammar: &'a LexicalGrammar,
    symbol: Symbol,
) -> (&'a str, VariableType) {
    match symbol.kind {
        SymbolType::End | SymbolType::EndOfNonTerminalExtra => ("end", VariableType::Hidden),
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
    }
}

struct SymbolVisibility {
    visible: bool,
    named: bool,
    supertype: bool,
}

fn symbol_visibility(
    syntax_grammar: &SyntaxGrammar,
    symbol: Symbol,
    kind: VariableType,
    default_aliases: &AliasMap,
) -> SymbolVisibility {
    if let Some(alias) = default_aliases.get(&symbol) {
        return SymbolVisibility {
            visible: true,
            named: alias.is_named,
            supertype: false,
        };
    }

    match kind {
        VariableType::Named => SymbolVisibility {
            visible: true,
            named: true,
            supertype: false,
        },
        VariableType::Anonymous => SymbolVisibility {
            visible: true,
            named: false,
            supertype: false,
        },
        VariableType::Hidden => SymbolVisibility {
            visible: false,
            named: true,
            supertype: syntax_grammar.supertype_symbols.contains(&symbol),
        },
        VariableType::Auxiliary => SymbolVisibility {
            visible: false,
            named: false,
            supertype: false,
        },
    }
}
