//! Private subset of Tree-sitter grammar lowering.
//!
//! Adapted from tree-sitter's MIT-licensed `tree-sitter-generate` crate.
//! This module exists only to derive Plotnik grammar metadata from grammar.json.

#![allow(dead_code, unexpected_cfgs)]

mod bitvec;
mod grammars;
mod nfa;
mod node_shapes;
mod prepare_grammar;
mod rules;

use super::types::{Precedence as PlotnikPrecedence, PrecedenceEntry as PlotnikPrecedenceEntry};
use super::types::{RawGrammar, Rule as PlotnikRule};
use crate::NodeShape;

use grammars::{InputGrammar, ReservedWordContext, Variable, VariableType};
use prepare_grammar::prepare_grammar;
use rules::{Precedence, Rule};

pub(super) fn node_shapes_for_raw(raw: &RawGrammar) -> Result<Vec<NodeShape>, String> {
    let input = input_grammar(raw);
    let (syntax_grammar, lexical_grammar, _inlines, aliases) =
        prepare_grammar(&input).map_err(|error| error.to_string())?;
    let variable_info = node_shapes::get_variable_info(&syntax_grammar, &lexical_grammar, &aliases)
        .map_err(|error| error.to_string())?;
    node_shapes::generate_node_shapes(&syntax_grammar, &lexical_grammar, &aliases, &variable_info)
        .map_err(|error| error.to_string())
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
        PlotnikRule::Blank => Rule::Blank,
        PlotnikRule::String(value) => Rule::String(value.clone()),
        PlotnikRule::Pattern { value, flags } => Rule::Pattern(
            value.clone(),
            flags.as_deref().map(filter_flags).unwrap_or_default(),
        ),
        PlotnikRule::Symbol(name) => Rule::NamedSymbol(name.clone()),
        PlotnikRule::Seq(members) => Rule::seq(members.iter().map(convert_rule).collect()),
        PlotnikRule::Choice(members) => Rule::choice(members.iter().map(convert_rule).collect()),
        PlotnikRule::Repeat(content) => {
            Rule::choice(vec![Rule::repeat(convert_rule(content)), Rule::Blank])
        }
        PlotnikRule::Repeat1(content) => Rule::repeat(convert_rule(content)),
        PlotnikRule::Field { name, content } => Rule::field(name.clone(), convert_rule(content)),
        PlotnikRule::Alias {
            content,
            value,
            named,
        } => Rule::alias(convert_rule(content), value.clone(), *named),
        PlotnikRule::Token(content) => Rule::token(convert_rule(content)),
        PlotnikRule::ImmediateToken(content) => Rule::immediate_token(convert_rule(content)),
        PlotnikRule::Prec { value, content } => {
            Rule::prec(convert_precedence(value), convert_rule(content))
        }
        PlotnikRule::PrecLeft { value, content } => {
            Rule::prec_left(convert_precedence(value), convert_rule(content))
        }
        PlotnikRule::PrecRight { value, content } => {
            Rule::prec_right(convert_precedence(value), convert_rule(content))
        }
        PlotnikRule::PrecDynamic { value, content } => {
            Rule::prec_dynamic(*value, convert_rule(content))
        }
        PlotnikRule::Reserved {
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
        PlotnikPrecedenceEntry::Name(name) => grammars::PrecedenceEntry::Name(name.clone()),
        PlotnikPrecedenceEntry::Symbol(name) => grammars::PrecedenceEntry::Symbol(name.clone()),
    }
}
