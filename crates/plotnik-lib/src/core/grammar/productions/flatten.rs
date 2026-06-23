use rustc_hash::FxHashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::super::{
    prepared::{
        ExtractedSyntaxGrammar, Production, ProductionStep, SyntaxGrammar, SyntaxVariable, Variable,
    },
    rules::{Alias, MetadataParams, Rule, Symbol},
};

pub type FlattenGrammarResult<T> = Result<T, FlattenGrammarError>;

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum FlattenGrammarError {
    #[error("No such reserved word set: {0}")]
    NoReservedWordSet(String),
    #[error(
        "The rule `{0}` matches the empty string.

Tree-sitter does not support syntactic rules that match the empty string
unless they are used only as the grammar's start rule.
"
    )]
    EmptyString(String),
    #[error("Rule `{0}` cannot be inlined because it contains a reference to itself")]
    RecursiveInline(String),
}

pub(in crate::core::grammar) fn flatten_grammar(
    grammar: ExtractedSyntaxGrammar,
) -> FlattenGrammarResult<SyntaxGrammar> {
    let reserved_word_set_names = grammar
        .reserved_word_sets
        .into_iter()
        .map(|set| set.name)
        .collect();

    let mut flattener = RuleFlattener::new(reserved_word_set_names);
    let variables = grammar
        .variables
        .into_iter()
        .map(|variable| flattener.flatten_variable(variable))
        .collect::<FlattenGrammarResult<Vec<_>>>()?;

    validate_productions(&variables, &grammar.variables_to_inline)?;

    Ok(SyntaxGrammar {
        variables,
        extra_symbols: grammar.extra_symbols,
        external_tokens: grammar.external_tokens,
        variables_to_inline: grammar.variables_to_inline,
        supertype_symbols: grammar.supertype_symbols,
        word_token: grammar.word_token,
    })
}

struct RuleFlattener {
    production: Production,
    reserved_word_set_names: FxHashSet<String>,
    alias_stack: Vec<Alias>,
    field_name_stack: Vec<String>,
}

impl RuleFlattener {
    fn new(reserved_word_set_names: FxHashSet<String>) -> Self {
        Self {
            production: Production { steps: Vec::new() },
            reserved_word_set_names,
            alias_stack: Vec::new(),
            field_name_stack: Vec::new(),
        }
    }

    fn flatten_variable(&mut self, variable: Variable) -> FlattenGrammarResult<SyntaxVariable> {
        let choices = extract_choices(variable.rule);
        let mut productions = Vec::with_capacity(choices.len());
        for rule in choices {
            let production = self.flatten_rule(rule)?;
            if !productions.contains(&production) {
                productions.push(production);
            }
        }
        Ok(SyntaxVariable {
            name: variable.name,
            kind: variable.kind,
            productions,
        })
    }

    fn flatten_rule(&mut self, rule: Rule) -> FlattenGrammarResult<Production> {
        self.production = Production::default();
        self.alias_stack.clear();
        self.field_name_stack.clear();
        self.apply(rule)?;
        Ok(std::mem::take(&mut self.production))
    }

    fn apply(&mut self, rule: Rule) -> FlattenGrammarResult<bool> {
        match rule {
            Rule::Seq(members) => {
                let mut result = false;
                for member in members {
                    result |= self.apply(member)?;
                }
                Ok(result)
            }
            Rule::Metadata { rule, params } => self.apply_metadata(*rule, params),
            Rule::Reserved { rule, context_name } => {
                if !self.reserved_word_set_names.contains(&context_name) {
                    Err(FlattenGrammarError::NoReservedWordSet(context_name))?;
                }
                self.apply(*rule)
            }
            Rule::Symbol(symbol) => {
                self.production.steps.push(ProductionStep {
                    symbol,
                    alias: self.alias_stack.last().cloned(),
                    field_name: self.field_name_stack.last().cloned(),
                });
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn apply_metadata(&mut self, rule: Rule, params: MetadataParams) -> FlattenGrammarResult<bool> {
        let pushed_alias = if let Some(alias) = params.alias {
            self.alias_stack.push(alias);
            true
        } else {
            false
        };

        let pushed_field_name = if let Some(field_name) = params.field_name {
            self.field_name_stack.push(field_name);
            true
        } else {
            false
        };

        let did_push = self.apply(rule)?;

        if pushed_alias {
            self.alias_stack.pop();
        }
        if pushed_field_name {
            self.field_name_stack.pop();
        }

        Ok(did_push)
    }
}

fn extract_choices(rule: Rule) -> Vec<Rule> {
    match rule {
        Rule::Seq(elements) => {
            let mut result = vec![Rule::Blank];
            for element in elements {
                let extraction = extract_choices(element);
                let mut next_result = Vec::with_capacity(result.len());
                for entry in result {
                    for extraction_entry in &extraction {
                        next_result.push(Rule::Seq(vec![entry.clone(), extraction_entry.clone()]));
                    }
                }
                result = next_result;
            }
            result
        }
        Rule::Choice(elements) => {
            let mut result = Vec::with_capacity(elements.len());
            for element in elements {
                for rule in extract_choices(element) {
                    result.push(rule);
                }
            }
            result
        }
        Rule::Metadata { rule, params } => extract_choices(*rule)
            .into_iter()
            .map(|rule| Rule::Metadata {
                rule: Box::new(rule),
                params: params.clone(),
            })
            .collect(),
        Rule::Reserved { rule, context_name } => extract_choices(*rule)
            .into_iter()
            .map(|rule| Rule::Reserved {
                rule: Box::new(rule),
                context_name: context_name.clone(),
            })
            .collect(),
        _ => vec![rule],
    }
}

fn validate_productions(
    variables: &[SyntaxVariable],
    variables_to_inline: &[Symbol],
) -> FlattenGrammarResult<()> {
    let used_symbols: FxHashSet<Symbol> = variables
        .iter()
        .flat_map(|v| &v.productions)
        .flat_map(|p| &p.steps)
        .map(|step| step.symbol)
        .collect();
    let inline_set: FxHashSet<Symbol> = variables_to_inline.iter().copied().collect();

    for (i, variable) in variables.iter().enumerate() {
        let symbol = Symbol::non_terminal(i);
        let used = used_symbols.contains(&symbol);

        for production in &variable.productions {
            if used && production.steps.is_empty() {
                Err(FlattenGrammarError::EmptyString(variable.name.clone()))?;
            }

            if inline_set.contains(&symbol)
                && production.steps.iter().any(|step| step.symbol == symbol)
            {
                Err(FlattenGrammarError::RecursiveInline(variable.name.clone()))?;
            }
        }
    }

    Ok(())
}
