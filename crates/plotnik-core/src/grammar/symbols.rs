use log::warn;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    prepared::{ReservedWordContext, ResolvedGrammar, Variable, VariableType},
    rules::{Rule, Symbol},
};

pub type InternSymbolsResult<T> = Result<T, InternSymbolsError>;

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum InternSymbolsError {
    #[error("A grammar's start rule must be visible.")]
    HiddenStartRule,
    #[error("Undefined symbol `{0}`")]
    Undefined(String),
    #[error("Undefined symbol `{0}` in grammar's supertypes array")]
    UndefinedSupertype(String),
    #[error("Undefined symbol `{0}` in grammar's conflicts array")]
    UndefinedConflict(String),
    #[error("Undefined symbol `{0}` as grammar's word token")]
    UndefinedWordToken(String),
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_symbols(
    source_variables: &[Variable],
    extra_rules: &[Rule],
    expected_conflicts: &[Vec<String>],
    external_rules: &[Rule],
    variables_to_inline: &[String],
    supertype_names: &[String],
    word_token_name: Option<&str>,
    reserved_word_sets: &[ReservedWordContext<Rule>],
) -> InternSymbolsResult<ResolvedGrammar> {
    let interner = Interner::new(source_variables, external_rules);

    if variable_type_for_name(&source_variables[0].name) == VariableType::Hidden {
        Err(InternSymbolsError::HiddenStartRule)?;
    }

    let mut variables = Vec::with_capacity(source_variables.len());
    for variable in source_variables {
        variables.push(Variable {
            name: variable.name.clone(),
            kind: variable_type_for_name(&variable.name),
            rule: interner.intern_rule(&variable.rule, Some(&variable.name))?,
        });
    }

    let mut external_tokens = Vec::with_capacity(external_rules.len());
    for external_token in external_rules {
        let rule = interner.intern_rule(external_token, None)?;
        let (name, kind) = if let Rule::NamedSymbol(name) = external_token {
            (name.clone(), variable_type_for_name(name))
        } else {
            (String::new(), VariableType::Anonymous)
        };
        external_tokens.push(Variable { name, kind, rule });
    }

    let mut extra_symbols = Vec::with_capacity(extra_rules.len());
    for extra_token in extra_rules {
        extra_symbols.push(interner.intern_rule(extra_token, None)?);
    }

    let mut supertype_symbols = Vec::with_capacity(supertype_names.len());
    for supertype_symbol_name in supertype_names {
        supertype_symbols.push(interner.intern_name(supertype_symbol_name).ok_or_else(|| {
            InternSymbolsError::UndefinedSupertype(supertype_symbol_name.clone())
        })?);
    }

    let mut reserved_words = Vec::with_capacity(reserved_word_sets.len());
    for reserved_word_set in reserved_word_sets {
        let mut interned_set = Vec::with_capacity(reserved_word_set.reserved_words.len());
        for rule in &reserved_word_set.reserved_words {
            interned_set.push(interner.intern_rule(rule, None)?);
        }
        reserved_words.push(ReservedWordContext {
            name: reserved_word_set.name.clone(),
            reserved_words: interned_set,
        });
    }

    for conflict in expected_conflicts {
        for name in conflict {
            interner
                .intern_name(name)
                .ok_or_else(|| InternSymbolsError::UndefinedConflict(name.clone()))?;
        }
    }

    let mut interned_inlines = Vec::new();
    for name in variables_to_inline {
        if let Some(symbol) = interner.intern_name(name) {
            interned_inlines.push(symbol);
        }
    }

    let word_token = if let Some(name) = word_token_name {
        Some(
            interner
                .intern_name(name)
                .ok_or_else(|| InternSymbolsError::UndefinedWordToken(name.to_string()))?,
        )
    } else {
        None
    };

    for (i, variable) in variables.iter_mut().enumerate() {
        if supertype_symbols.contains(&Symbol::non_terminal(i)) {
            // Supertypes group concrete rules but are not emitted as concrete CST nodes.
            variable.kind = VariableType::Hidden;
        }
    }

    Ok(ResolvedGrammar {
        variables,
        extra_symbols,
        external_tokens,
        variables_to_inline: interned_inlines,
        supertype_symbols,
        word_token,
        reserved_word_sets: reserved_words,
    })
}

struct Interner<'a> {
    /// Name → Symbol. Variables take priority over external rules, and the
    /// lowest index wins within each group, matching the original ordered scan.
    name_map: FxHashMap<&'a str, Symbol>,
}

impl<'a> Interner<'a> {
    fn new(variables: &'a [Variable], external_rules: &'a [Rule]) -> Self {
        let mut name_map = FxHashMap::default();
        for (i, variable) in variables.iter().enumerate() {
            name_map
                .entry(variable.name.as_str())
                .or_insert_with(|| Symbol::non_terminal(i));
        }
        for (i, external_token) in external_rules.iter().enumerate() {
            if let Rule::NamedSymbol(name) = external_token {
                name_map
                    .entry(name.as_str())
                    .or_insert_with(|| Symbol::external(i));
            }
        }
        Self { name_map }
    }

    fn intern_rule(&self, rule: &Rule, name: Option<&str>) -> InternSymbolsResult<Rule> {
        match rule {
            Rule::Choice(elements) => {
                Self::check_single(elements, name, "choice");
                let mut result = Vec::with_capacity(elements.len());
                for element in elements {
                    result.push(self.intern_rule(element, name)?);
                }
                Ok(Rule::Choice(result))
            }
            Rule::Seq(elements) => {
                Self::check_single(elements, name, "seq");
                let mut result = Vec::with_capacity(elements.len());
                for element in elements {
                    result.push(self.intern_rule(element, name)?);
                }
                Ok(Rule::Seq(result))
            }
            Rule::Repeat(content) => Ok(Rule::Repeat(Box::new(self.intern_rule(content, name)?))),
            Rule::Metadata { rule, params } => Ok(Rule::Metadata {
                rule: Box::new(self.intern_rule(rule, name)?),
                params: params.clone(),
            }),
            Rule::Reserved { rule, context_name } => Ok(Rule::Reserved {
                rule: Box::new(self.intern_rule(rule, name)?),
                context_name: context_name.clone(),
            }),
            Rule::NamedSymbol(name) => self.intern_name(name).map_or_else(
                || Err(InternSymbolsError::Undefined(name.clone())),
                |symbol| Ok(Rule::Symbol(symbol)),
            ),
            _ => Ok(rule.clone()),
        }
    }

    fn intern_name(&self, symbol: &str) -> Option<Symbol> {
        self.name_map.get(symbol).copied()
    }

    // In the case of a seq or choice rule of 1 element in a hidden rule, weird
    // inconsistent behavior with queries can occur. So we should warn the user about it.
    fn check_single(elements: &[Rule], name: Option<&str>, kind: &str) {
        if elements.len() == 1 && matches!(elements[0], Rule::String(_) | Rule::Pattern(_, _)) {
            warn!(
                "rule {} contains a `{kind}` rule with a single element. This is unnecessary.",
                name.unwrap_or_default()
            );
        }
    }
}

fn variable_type_for_name(name: &str) -> VariableType {
    if name.starts_with('_') {
        VariableType::Hidden
    } else {
        VariableType::Named
    }
}
