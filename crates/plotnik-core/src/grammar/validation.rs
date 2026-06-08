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

pub type ValidatePrecedenceResult<T> = Result<T, ValidatePrecedenceError>;

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

/// Check for indirect recursion cycles in the grammar that can cause infinite loops while
/// parsing. An indirect recursion cycle occurs when a non-terminal can derive itself through
/// a chain of single-symbol productions (e.g., A -> B, B -> A).
pub(super) fn validate_indirect_recursion(
    variables: &[Variable],
) -> Result<(), IndirectRecursionError> {
    let mut epsilon_transitions: IndexMap<&str, BTreeSet<String>> = IndexMap::new();

    for variable in variables {
        let productions = get_single_symbol_productions(&variable.rule);
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
            get_cycle(start_symbol, &epsilon_transitions, &mut visited, &mut path)
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

fn get_single_symbol_productions(rule: &Rule) -> BTreeSet<String> {
    match rule {
        Rule::NamedSymbol(name) => BTreeSet::from([name.clone()]),
        Rule::Choice(choices) => choices
            .iter()
            .flat_map(get_single_symbol_productions)
            .collect(),
        Rule::Metadata { rule, .. } => get_single_symbol_productions(rule),
        _ => BTreeSet::new(),
    }
}

/// Perform a depth-first search to detect cycles in single state transitions.
fn get_cycle<'a>(
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
            if let Some(cycle) = get_cycle(next, transitions, visited, path) {
                return Some(cycle);
            }
        }
    }

    path.pop();
    None
}

/// Check that all of the named precedences used in the grammar are declared
/// within the `precedences` lists, and also that there are no conflicting
/// precedence orderings declared in those lists.
pub(super) fn validate_precedences(
    variables: &[Variable],
    precedence_orderings: &[Vec<PrecedenceEntry>],
) -> ValidatePrecedenceResult<()> {
    // Check that no rule contains a named precedence that is not present in
    // any of the `precedences` lists.
    fn validate(
        rule_name: &str,
        rule: &Rule,
        names: &FxHashSet<&String>,
    ) -> ValidatePrecedenceResult<()> {
        match rule {
            Rule::Repeat(rule) => validate(rule_name, rule, names),
            Rule::Seq(elements) | Rule::Choice(elements) => elements
                .iter()
                .try_for_each(|e| validate(rule_name, e, names)),
            Rule::Metadata { rule, params } => {
                if let Precedence::Name(n) = &params.precedence
                    && !names.contains(n)
                {
                    Err(UndeclaredPrecedenceError {
                        precedence: n.clone(),
                        rule: rule_name.to_string(),
                    })?;
                }
                validate(rule_name, rule, names)?;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    // For any two precedence names `a` and `b`, if `a` comes before `b`
    // in some list, then it cannot come *after* `b` in any list.
    let mut pairs = FxHashMap::default();
    for list in precedence_orderings {
        for (i, mut entry1) in list.iter().enumerate() {
            for mut entry2 in list.iter().skip(i + 1) {
                if entry2 == entry1 {
                    continue;
                }
                let mut ordering = Ordering::Greater;
                if entry1 > entry2 {
                    ordering = Ordering::Less;
                    mem::swap(&mut entry1, &mut entry2);
                }
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
        validate(&variable.name, &variable.rule, &precedence_names)?;
    }

    Ok(())
}
