//! Private subset of Tree-sitter grammar lowering.
//!
//! Adapted from tree-sitter's MIT-licensed `tree-sitter-generate` crate.
//! This module exists only to derive Plotnik grammar metadata from grammar.json.

#![allow(dead_code, unexpected_cfgs)]

mod bitvec;
mod build_tables;
mod grammars;
mod nfa;
mod node_shapes;
mod prepare_grammar;
#[cfg(plotnik_grammar_profile)]
pub(super) mod profile;
mod rules;
mod tables;

use super::raw::{
    RawGrammar, RawPrecedence as PlotnikPrecedence, RawPrecedenceEntry as PlotnikPrecedenceEntry,
    RawRule as PlotnikRule,
};
use super::types::NodeShape;

use grammars::{
    InputGrammar, LexicalGrammar, ReservedWordContext, SyntaxGrammar, Variable, VariableType,
};
use prepare_grammar::prepare_grammar;
use rules::{Alias, AliasMap, Precedence, Rule, Symbol, SymbolType};
use tables::ParseTable;

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

pub(super) fn metadata_for_raw(raw: &RawGrammar) -> Result<GrammarMetadata, String> {
    let input = input_grammar(raw);
    let (syntax_grammar, lexical_grammar, inlines, aliases) =
        prepare_grammar(&input).map_err(|error| error.to_string())?;
    let variable_info = node_shapes::get_variable_info(&syntax_grammar, &lexical_grammar, &aliases)
        .map_err(|error| error.to_string())?;
    let node_shapes = node_shapes::generate_node_shapes(
        &syntax_grammar,
        &lexical_grammar,
        &aliases,
        &variable_info,
    )
    .map_err(|error| error.to_string())?;
    let parse_table = build_tables::build_metadata_tables(
        &syntax_grammar,
        &lexical_grammar,
        &variable_info,
        &inlines,
    )
    .map(|tables| tables.parse_table)
    .map_err(|error| error.to_string())?;

    Ok(GrammarMetadata {
        node_shapes,
        symbols: derive_symbols(&syntax_grammar, &lexical_grammar, &parse_table, &aliases),
        fields: derive_fields(&parse_table),
    })
}

fn input_grammar(raw: &RawGrammar) -> InputGrammar {
    InputGrammar {
        name: raw.name.clone(),
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
            .map(|(name, rules)| ReservedWordContext {
                name: name.clone(),
                reserved_words: rules.iter().map(convert_rule).collect(),
            })
            .collect(),
    }
}

fn convert_rule(rule: &PlotnikRule) -> Rule {
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

fn convert_precedence_entry(entry: &PlotnikPrecedenceEntry) -> grammars::PrecedenceEntry {
    match entry {
        PlotnikPrecedenceEntry::STRING { value } => grammars::PrecedenceEntry::Name(value.clone()),
        PlotnikPrecedenceEntry::SYMBOL { name } => grammars::PrecedenceEntry::Symbol(name.clone()),
    }
}

fn derive_fields(parse_table: &ParseTable) -> Vec<FieldSymbol> {
    let mut field_names = Vec::<String>::new();
    for production_info in &parse_table.production_infos {
        for field_name in production_info.field_map.keys() {
            if let Err(index) = field_names.binary_search(field_name) {
                field_names.insert(index, field_name.clone());
            }
        }
    }

    field_names
        .into_iter()
        .enumerate()
        .map(|(index, name)| FieldSymbol {
            id: u16::try_from(index + 1).expect("tree-sitter field IDs fit in u16"),
            name,
        })
        .collect()
}

fn derive_symbols(
    syntax_grammar: &SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
    parse_table: &ParseTable,
    default_aliases: &AliasMap,
) -> Vec<NodeSymbol> {
    let symbol_ids = symbol_ids(parse_table);
    let symbol_map = public_symbol_map(
        syntax_grammar,
        lexical_grammar,
        parse_table,
        default_aliases,
    );
    let unique_aliases = unique_aliases(
        syntax_grammar,
        lexical_grammar,
        parse_table,
        default_aliases,
        &symbol_ids,
        &symbol_map,
    );

    let mut symbols = Vec::new();
    for symbol in &parse_table.symbols {
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
            type_name: type_name.to_string(),
            named,
            visible,
            supertype,
        });
    }

    let first_alias_id = symbol_ids
        .values()
        .copied()
        .max()
        .expect("tree-sitter parse table includes end symbol")
        + 1;
    for (index, alias) in unique_aliases.iter().enumerate() {
        symbols.push(NodeSymbol {
            id: first_alias_id + u16::try_from(index).expect("tree-sitter alias IDs fit in u16"),
            type_name: alias.value.clone(),
            named: alias.is_named,
            visible: true,
            supertype: false,
        });
    }

    symbols
}

fn symbol_ids(parse_table: &ParseTable) -> rustc_hash::FxHashMap<Symbol, u16> {
    let mut ids = rustc_hash::FxHashMap::default();
    ids.insert(Symbol::end(), 0);

    let mut next_id = 1u16;
    for symbol in &parse_table.symbols {
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
    parse_table: &ParseTable,
    default_aliases: &AliasMap,
) -> rustc_hash::FxHashMap<Symbol, Symbol> {
    let mut symbol_map = rustc_hash::FxHashMap::default();

    for symbol in &parse_table.symbols {
        let mut mapping = *symbol;

        if let Some(alias) = default_aliases.get(symbol) {
            let kind = alias.kind();
            for other_symbol in &parse_table.symbols {
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
            for other_symbol in &parse_table.symbols {
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
    parse_table: &ParseTable,
    default_aliases: &AliasMap,
    symbol_ids: &rustc_hash::FxHashMap<Symbol, u16>,
    symbol_map: &rustc_hash::FxHashMap<Symbol, Symbol>,
) -> Vec<Alias> {
    let mut aliases = Vec::new();

    for production_info in &parse_table.production_infos {
        for alias in production_info.alias_sequence.iter().flatten() {
            let has_existing_symbol = symbols_for_alias(
                syntax_grammar,
                lexical_grammar,
                parse_table,
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
    }

    aliases
}

fn symbols_for_alias(
    syntax_grammar: &SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
    parse_table: &ParseTable,
    default_aliases: &AliasMap,
    alias: &Alias,
) -> Vec<Symbol> {
    parse_table
        .symbols
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
