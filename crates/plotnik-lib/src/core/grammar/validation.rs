use std::{
    cmp::Ordering,
    collections::{BTreeSet, hash_map},
    mem,
};

use indexmap::IndexMap;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    prepared::{PrecedenceEntry, Variable},
    rules::{Precedence, Rule},
};

#[derive(Debug, Error, Serialize, Deserialize)]
#[error(transparent)]
pub enum ValidatePrecedenceError {
    Undeclared(#[from] UndeclaredPrecedenceError),
    Ordering(#[from] ConflictingPrecedenceOrderingError),
}

#[derive(Debug, Error, Serialize, Deserialize)]
pub struct IndirectRecursionError(pub Vec<String>);

impl std::fmt::Display for IndirectRecursionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Grammar contains an indirectly recursive rule: ")?;
        for (i, symbol) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, " -> ")?;
            }
            write!(f, "{symbol}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Error, Serialize, Deserialize)]
pub struct UndeclaredPrecedenceError {
    pub precedence: String,
    pub rule: String,
}

impl std::fmt::Display for UndeclaredPrecedenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Undeclared precedence '{}' in rule '{}'",
            self.precedence, self.rule
        )?;
        Ok(())
    }
}

#[derive(Debug, Error, Serialize, Deserialize)]
pub struct ConflictingPrecedenceOrderingError {
    pub precedence_1: String,
    pub precedence_2: String,
}

impl std::fmt::Display for ConflictingPrecedenceOrderingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Conflicting orderings for precedences {} and {}",
            self.precedence_1, self.precedence_2
        )?;
        Ok(())
    }
}

/// Detects cycles through chains of single-symbol productions (A→B, B→A), which cause
/// infinite loops at parse time because no input is consumed between transitions.
pub(super) fn validate_indirect_recursion(
    variables: &[Variable],
) -> Result<(), IndirectRecursionError> {
    let mut epsilon_transitions: IndexMap<&str, BTreeSet<String>> = IndexMap::new();

    for variable in variables {
        let productions = single_symbol_productions(&variable.rule);
        // Filter out rules that *directly* reference themselves, as this doesn't
        // cause a parsing loop.
        let filtered: BTreeSet<String> = productions
            .into_iter()
            .filter(|s| s != &variable.name)
            .collect();
        epsilon_transitions.insert(variable.name.as_str(), filtered);
    }

    for start_symbol in epsilon_transitions.keys() {
        let mut visited = BTreeSet::new();
        let mut path = Vec::new();
        if let Some((start_idx, end_idx)) =
            find_cycle(start_symbol, &epsilon_transitions, &mut visited, &mut path)
        {
            let cycle_symbols = path[start_idx..=end_idx]
                .iter()
                .map(|s| (*s).to_string())
                .collect();
            return Err(IndirectRecursionError(cycle_symbols));
        }
    }

    Ok(())
}

fn single_symbol_productions(rule: &Rule) -> BTreeSet<String> {
    match rule {
        Rule::NamedSymbol(name) => BTreeSet::from([name.clone()]),
        Rule::Choice(choices) => choices.iter().flat_map(single_symbol_productions).collect(),
        Rule::Metadata { rule, .. } => single_symbol_productions(rule),
        _ => BTreeSet::new(),
    }
}

fn find_cycle<'a>(
    current: &'a str,
    transitions: &'a IndexMap<&'a str, BTreeSet<String>>,
    visited: &mut BTreeSet<&'a str>,
    path: &mut Vec<&'a str>,
) -> Option<(usize, usize)> {
    if let Some(first_idx) = path.iter().position(|s| *s == current) {
        path.push(current);
        return Some((first_idx, path.len() - 1));
    }

    if visited.contains(current) {
        return None;
    }

    path.push(current);
    visited.insert(current);

    if let Some(next_symbols) = transitions.get(current) {
        for next in next_symbols {
            if let Some(cycle) = find_cycle(next, transitions, visited, path) {
                return Some(cycle);
            }
        }
    }

    path.pop();
    None
}

pub(super) fn validate_precedences(
    variables: &[Variable],
    precedence_orderings: &[Vec<PrecedenceEntry>],
) -> Result<(), ValidatePrecedenceError> {
    validate_conflicting_precedence_orderings(precedence_orderings)?;
    validate_declared_precedences(variables, precedence_orderings)?;
    Ok(())
}

// For any two precedence names `a` and `b`, if `a` comes before `b`
// in some list, then it cannot come *after* `b` in any list.
fn validate_conflicting_precedence_orderings(
    precedence_orderings: &[Vec<PrecedenceEntry>],
) -> Result<(), ValidatePrecedenceError> {
    let mut pairs = FxHashMap::default();
    for list in precedence_orderings {
        for (i, mut entry1) in list.iter().enumerate() {
            for mut entry2 in list.iter().skip(i + 1) {
                if entry2 == entry1 {
                    continue;
                }

                let ordering = if entry1 > entry2 {
                    mem::swap(&mut entry1, &mut entry2);
                    Ordering::Less
                } else {
                    Ordering::Greater
                };

                match pairs.entry((entry1, entry2)) {
                    hash_map::Entry::Vacant(e) => {
                        e.insert(ordering);
                    }
                    hash_map::Entry::Occupied(e) => {
                        if e.get() != &ordering {
                            Err(ConflictingPrecedenceOrderingError {
                                precedence_1: entry1.to_string(),
                                precedence_2: entry2.to_string(),
                            })?;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn validate_declared_precedences(
    variables: &[Variable],
    precedence_orderings: &[Vec<PrecedenceEntry>],
) -> Result<(), ValidatePrecedenceError> {
    let precedence_names = precedence_orderings
        .iter()
        .flat_map(|l| l.iter())
        .filter_map(|p| {
            if let PrecedenceEntry::Name(n) = p {
                Some(n)
            } else {
                None
            }
        })
        .collect::<FxHashSet<&String>>();

    for variable in variables {
        validate_declared_precedences_in_rule(&variable.name, &variable.rule, &precedence_names)?;
    }

    Ok(())
}

fn validate_declared_precedences_in_rule(
    rule_name: &str,
    rule: &Rule,
    names: &FxHashSet<&String>,
) -> Result<(), ValidatePrecedenceError> {
    match rule {
        Rule::Repeat(rule) => validate_declared_precedences_in_rule(rule_name, rule, names),
        Rule::Seq(elements) | Rule::Choice(elements) => elements
            .iter()
            .try_for_each(|e| validate_declared_precedences_in_rule(rule_name, e, names)),
        Rule::Metadata { rule, params } => {
            if let Precedence::Name(n) = &params.precedence
                && !names.contains(n)
            {
                Err(UndeclaredPrecedenceError {
                    precedence: n.clone(),
                    rule: rule_name.to_string(),
                })?;
            }
            validate_declared_precedences_in_rule(rule_name, rule, names)?;
            Ok(())
        }
        _ => Ok(()),
    }
}
