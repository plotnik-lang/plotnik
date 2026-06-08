//! Raw grammar lowering helpers.

use std::collections::BTreeSet;

use rustc_hash::{FxHashMap, FxHashSet};

use super::raw::{
    RawPrecedence as PlotnikPrecedence, RawPrecedenceEntry as PlotnikPrecedenceEntry,
    RawRule as PlotnikRule,
};
use super::{
    node_shapes,
    prepared::{
        InlinedProductionMap, LexicalGrammar, PrecedenceEntry, Production, ReservedWordContext,
        SyntaxGrammar, Variable, VariableType,
    },
    rules::{Alias, AliasMap, Precedence, Rule, Symbol, SymbolType},
    types::{FieldSymbol, NodeSymbol},
};

#[allow(clippy::too_many_arguments)]
pub(super) fn retain_reachable_rules(
    variables: &mut Vec<Variable>,
    extra_symbols: &mut Vec<Rule>,
    expected_conflicts: &mut Vec<Vec<String>>,
    precedence_orderings: &mut Vec<Vec<PrecedenceEntry>>,
    external_tokens: &mut Vec<Rule>,
    variables_to_inline: &mut Vec<String>,
    supertype_symbols: &mut Vec<String>,
    word_token: Option<&str>,
    reserved_words: &mut [ReservedWordContext<Rule>],
) {
    let used = reachable_rule_names(
        variables,
        word_token,
        extra_symbols,
        external_tokens,
        reserved_words,
    );
    let dropped = variables
        .iter()
        .filter(|variable| !used.contains(variable.name.as_str()))
        .map(|variable| variable.name.clone())
        .collect::<Vec<_>>();

    variables.retain(|variable| used.contains(variable.name.as_str()));

    for name in &dropped {
        expected_conflicts.retain(|conflict| !conflict.contains(name));
        supertype_symbols.retain(|symbol| symbol != name);
        variables_to_inline.retain(|symbol| symbol != name);
        extra_symbols.retain(|rule| !rule_is_referenced(rule, name, true));
        external_tokens.retain(|rule| !rule_is_referenced(rule, name, true));
        precedence_orderings.retain(|ordering| {
            !ordering
                .iter()
                .any(|entry| matches!(entry, PrecedenceEntry::Symbol(symbol) if symbol == name))
        });
        for context in reserved_words.iter_mut() {
            context
                .reserved_words
                .retain(|rule| !rule_is_referenced(rule, name, false));
        }
    }
}

fn reachable_rule_names<'a>(
    variables: &'a [Variable],
    word_token: Option<&'a str>,
    extra_symbols: &'a [Rule],
    external_tokens: &'a [Rule],
    reserved_words: &'a [ReservedWordContext<Rule>],
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
    match rule {
        Rule::NamedSymbol(name) => {
            if !skip_top_level {
                out.push(name.as_str());
            }
        }
        Rule::Choice(rules) | Rule::Seq(rules) => {
            for rule in rules {
                collect_referenced_names(rule, false, out);
            }
        }
        Rule::Metadata { rule, .. } | Rule::Reserved { rule, .. } => {
            collect_referenced_names(rule, skip_top_level, out);
        }
        Rule::Repeat(rule) => collect_referenced_names(rule, false, out),
        Rule::Blank | Rule::String(_) | Rule::Pattern(_, _) | Rule::Symbol(_) => {}
    }
}

fn rule_is_referenced(rule: &Rule, target: &str, is_external: bool) -> bool {
    match rule {
        Rule::NamedSymbol(name) => name == target && !is_external,
        Rule::Choice(rules) | Rule::Seq(rules) => rules
            .iter()
            .any(|rule| rule_is_referenced(rule, target, false)),
        Rule::Metadata { rule, .. } | Rule::Reserved { rule, .. } => {
            rule_is_referenced(rule, target, is_external)
        }
        Rule::Repeat(rule) => rule_is_referenced(rule, target, false),
        Rule::Blank | Rule::String(_) | Rule::Pattern(_, _) | Rule::Symbol(_) => false,
    }
}

pub(super) fn convert_rule(rule: &PlotnikRule) -> Rule {
    match rule {
        PlotnikRule::BLANK => Rule::Blank,
        PlotnikRule::STRING { value } => Rule::String(value.clone()),
        PlotnikRule::PATTERN { value, flags } => Rule::Pattern(
            value.clone(),
            flags.as_deref().map(filter_flags).unwrap_or_default(),
        ),
        PlotnikRule::SYMBOL { name } => Rule::NamedSymbol(name.clone()),
        PlotnikRule::SEQ { members } => Rule::seq(members.iter().map(convert_rule).collect()),
        PlotnikRule::CHOICE { members } => Rule::choice(members.iter().map(convert_rule).collect()),
        PlotnikRule::REPEAT { content } => {
            Rule::choice(vec![Rule::repeat(convert_rule(content)), Rule::Blank])
        }
        PlotnikRule::REPEAT1 { content } => Rule::repeat(convert_rule(content)),
        PlotnikRule::FIELD { name, content } => Rule::field(name.clone(), convert_rule(content)),
        PlotnikRule::ALIAS {
            content,
            value,
            named,
        } => Rule::alias(convert_rule(content), value.clone(), *named),
        PlotnikRule::TOKEN { content } => Rule::token(convert_rule(content)),
        PlotnikRule::IMMEDIATE_TOKEN { content } => Rule::immediate_token(convert_rule(content)),
        PlotnikRule::PREC { value, content } => {
            Rule::prec(convert_precedence(value), convert_rule(content))
        }
        PlotnikRule::PREC_LEFT { value, content } => {
            Rule::prec_left(convert_precedence(value), convert_rule(content))
        }
        PlotnikRule::PREC_RIGHT { value, content } => {
            Rule::prec_right(convert_precedence(value), convert_rule(content))
        }
        PlotnikRule::PREC_DYNAMIC { value, content } => {
            Rule::prec_dynamic(*value, convert_rule(content))
        }
        PlotnikRule::RESERVED {
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

fn convert_precedence(precedence: &PlotnikPrecedence) -> Precedence {
    match precedence {
        PlotnikPrecedence::Integer(value) => Precedence::Integer(*value),
        PlotnikPrecedence::Name(name) => Precedence::Name(name.clone()),
    }
}

pub(super) fn convert_precedence_entry(entry: &PlotnikPrecedenceEntry) -> PrecedenceEntry {
    match entry {
        PlotnikPrecedenceEntry::STRING { value } => PrecedenceEntry::Name(value.clone()),
        PlotnikPrecedenceEntry::SYMBOL { name } => PrecedenceEntry::Symbol(name.clone()),
    }
}

pub(super) fn derive_fields(
    syntax_grammar: &SyntaxGrammar,
    inlines: &InlinedProductionMap,
    variable_info: &[node_shapes::VariableInfo],
) -> Vec<FieldSymbol> {
    let mut field_names = BTreeSet::<String>::new();
    for_each_metadata_production(syntax_grammar, inlines, |production| {
        collect_field_names(production, syntax_grammar, variable_info, &mut field_names);
    });

    field_names
        .into_iter()
        .enumerate()
        .map(|(index, name)| FieldSymbol {
            id: u16::try_from(index + 1).expect("tree-sitter field IDs fit in u16"),
            name,
        })
        .collect()
}

fn collect_field_names(
    production: &Production,
    syntax_grammar: &SyntaxGrammar,
    variable_info: &[node_shapes::VariableInfo],
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
) -> Vec<NodeSymbol> {
    let symbol_order = derive_symbol_order(syntax_grammar, lexical_grammar);
    let symbol_ids = symbol_ids(&symbol_order);
    let symbol_map = public_symbol_map(
        syntax_grammar,
        lexical_grammar,
        &symbol_order,
        default_aliases,
    );
    let unique_aliases = unique_aliases(
        syntax_grammar,
        lexical_grammar,
        inlines,
        &symbol_order,
        default_aliases,
        &symbol_ids,
        &symbol_map,
    );

    let mut symbols = Vec::new();
    for symbol in &symbol_order {
        let public_id = symbol_ids[&symbol_map[symbol]];
        let (type_name, kind) = default_aliases.get(symbol).map_or_else(
            || metadata_for_symbol(syntax_grammar, lexical_grammar, *symbol),
            |alias| (alias.value.as_str(), alias.kind()),
        );
        let (visible, named, supertype) =
            symbol_visibility(syntax_grammar, *symbol, kind, default_aliases);

        if public_id == 0 {
            continue;
        }

        symbols.push(NodeSymbol {
            id: public_id,
            type_name: public_node_type_name(type_name),
            named,
            visible,
            supertype,
        });
    }

    let first_alias_id = symbol_ids
        .values()
        .copied()
        .max()
        .expect("tree-sitter symbol order includes end symbol")
        + 1;
    for (index, alias) in unique_aliases.iter().enumerate() {
        symbols.push(NodeSymbol {
            id: first_alias_id + u16::try_from(index).expect("tree-sitter alias IDs fit in u16"),
            type_name: public_node_type_name(&alias.value),
            named: alias.is_named,
            visible: true,
            supertype: false,
        });
    }

    symbols
}

fn public_node_type_name(name: &str) -> String {
    name.split('\0').next().unwrap_or(name).to_string()
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
            symbols.insert(1, symbol);
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

fn unique_aliases(
    syntax_grammar: &SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
    inlines: &InlinedProductionMap,
    symbols: &[Symbol],
    default_aliases: &AliasMap,
    symbol_ids: &rustc_hash::FxHashMap<Symbol, u16>,
    symbol_map: &rustc_hash::FxHashMap<Symbol, Symbol>,
) -> Vec<Alias> {
    let mut aliases = Vec::new();

    for_each_metadata_production(syntax_grammar, inlines, |production| {
        for alias in production
            .steps
            .iter()
            .filter_map(|step| step.alias.as_ref())
        {
            let has_existing_symbol = symbols_for_alias(
                syntax_grammar,
                lexical_grammar,
                symbols,
                default_aliases,
                alias,
            )
            .first()
            .and_then(|symbol| symbol_map.get(symbol))
            .is_some_and(|symbol| symbol_ids.contains_key(symbol));

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

fn symbols_for_alias(
    syntax_grammar: &SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
    symbols: &[Symbol],
    default_aliases: &AliasMap,
    alias: &Alias,
) -> Vec<Symbol> {
    symbols
        .iter()
        .copied()
        .filter(|symbol| {
            default_aliases.get(symbol).map_or_else(
                || {
                    let (name, kind) =
                        metadata_for_symbol(syntax_grammar, lexical_grammar, *symbol);
                    name == alias.value && kind == alias.kind()
                },
                |default_alias| default_alias == alias,
            )
        })
        .collect()
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

fn symbol_visibility(
    syntax_grammar: &SyntaxGrammar,
    symbol: Symbol,
    kind: VariableType,
    default_aliases: &AliasMap,
) -> (bool, bool, bool) {
    if let Some(alias) = default_aliases.get(&symbol) {
        return (true, alias.is_named, false);
    }

    match kind {
        VariableType::Named => (true, true, false),
        VariableType::Anonymous => (true, false, false),
        VariableType::Hidden => (
            false,
            true,
            syntax_grammar.supertype_symbols.contains(&symbol),
        ),
        VariableType::Auxiliary => (false, false, false),
    }
}
