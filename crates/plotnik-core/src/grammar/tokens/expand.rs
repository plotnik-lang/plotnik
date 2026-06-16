use regex_syntax::{
    ParserBuilder,
    hir::{Class, Hir, HirKind},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::super::{
    nfa::{CharacterSet, Nfa, NfaState},
    prepared::{ExtractedLexicalGrammar, LexicalGrammar, LexicalVariable},
    rules::{Precedence, Rule},
};

struct NfaBuilder {
    nfa: Nfa,
    is_sep: bool,
    precedence_stack: Vec<i32>,
}

pub type ExpandTokensResult<T> = Result<T, ExpandTokensError>;

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum ExpandTokensError {
    #[error(
        "The rule `{0}` matches the empty string.
Tree-sitter does not support syntactic rules that match the empty string
unless they are used only as the grammar's start rule.
"
    )]
    EmptyString(String),
    #[error(transparent)]
    Processing(ExpandTokensProcessingError),
    #[error(transparent)]
    ExpandRule(ExpandRuleError),
}

#[derive(Debug, Error, Serialize, Deserialize)]
pub struct ExpandTokensProcessingError {
    rule: String,
    error: ExpandRuleError,
}

impl std::fmt::Display for ExpandTokensProcessingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Error processing rule {}: Grammar error: Unexpected rule {:?}",
            self.rule, self.error
        )?;
        Ok(())
    }
}

const fn get_completion_precedence(rule: &Rule) -> i32 {
    if let Rule::Metadata { params, .. } = rule
        && let Precedence::Integer(p) = params.precedence
    {
        return p;
    }
    0
}

pub fn expand_tokens(mut grammar: ExtractedLexicalGrammar) -> ExpandTokensResult<LexicalGrammar> {
    let mut builder = NfaBuilder {
        nfa: Nfa::new(),
        is_sep: true,
        precedence_stack: vec![0],
    };

    let separator_rule = if grammar.separators.is_empty() {
        Rule::Blank
    } else {
        grammar.separators.push(Rule::Blank);
        Rule::repeat(Rule::choice(grammar.separators))
    };

    let mut variables = Vec::with_capacity(grammar.variables.len());
    for (i, variable) in grammar.variables.into_iter().enumerate() {
        if variable.rule.is_empty() {
            Err(ExpandTokensError::EmptyString(variable.name.clone()))?;
        }

        let is_immediate_token = match &variable.rule {
            Rule::Metadata { params, .. } => params.is_main_token,
            _ => false,
        };

        builder.is_sep = false;
        builder.nfa.states.push(NfaState::Accept {
            variable_index: i,
            precedence: get_completion_precedence(&variable.rule),
        });
        let last_state_id = builder.nfa.last_state_id();
        builder
            .expand_rule(&variable.rule, last_state_id)
            .map_err(|e| {
                ExpandTokensError::Processing(ExpandTokensProcessingError {
                    rule: variable.name.clone(),
                    error: e,
                })
            })?;

        if !is_immediate_token {
            builder.is_sep = true;
            let last_state_id = builder.nfa.last_state_id();
            builder
                .expand_rule(&separator_rule, last_state_id)
                .map_err(ExpandTokensError::ExpandRule)?;
        }

        variables.push(LexicalVariable {
            name: variable.name,
            kind: variable.kind,
        });
    }

    Ok(LexicalGrammar { variables })
}

pub type ExpandRuleResult<T> = Result<T, ExpandRuleError>;

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum ExpandRuleError {
    #[error("Grammar error: Unexpected rule {0:?}")]
    UnexpectedRule(Rule),
    #[error("{0}")]
    Parse(String),
    #[error(transparent)]
    ExpandRegex(ExpandRegexError),
}

pub type ExpandRegexResult<T> = Result<T, ExpandRegexError>;

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum ExpandRegexError {
    #[error("{0}")]
    Utf8(String),
    #[error("Regex error: Assertions are not supported")]
    Assertion,
}

fn normalize_tree_sitter_regex(regex: &str) -> String {
    // With unicode enabled, `\w`, `\s` and `\d` expand to character sets that are much
    // larger than intended, so replace them with Tree-sitter's ASCII-oriented sets.
    // Use `\p{L}`, `\p{Z}` and `\p{N}` when the full unicode ranges are intended.
    regex
        .replace(r"\w", r"[0-9A-Za-z_]")
        .replace(r"\s", r"[\t-\r ]")
        .replace(r"\d", r"[0-9]")
        .replace(r"\W", r"[^0-9A-Za-z_]")
        .replace(r"\S", r"[^\t-\r ]")
        .replace(r"\D", r"[^0-9]")
}

fn is_case_folded_long_s_set(chars: &CharacterSet) -> bool {
    let case_folded_long_s_ranges = ['s'..='s', 'S'..='S', 'ſ'..='ſ'];
    chars.range_count() == case_folded_long_s_ranges.len()
        && chars
            .ranges()
            .all(|range| case_folded_long_s_ranges.contains(&range))
}

impl NfaBuilder {
    fn expand_rule(&mut self, rule: &Rule, mut next_state_id: u32) -> ExpandRuleResult<bool> {
        match rule {
            Rule::Pattern(s, f) => {
                let s = normalize_tree_sitter_regex(s);
                let mut parser = ParserBuilder::new()
                    .case_insensitive(f.contains('i'))
                    .unicode(true)
                    .utf8(false)
                    .build();
                let hir = parser
                    .parse(&s)
                    .map_err(|e| ExpandRuleError::Parse(e.to_string()))?;
                self.expand_regex(&hir, next_state_id)
                    .map_err(ExpandRuleError::ExpandRegex)
            }
            Rule::String(s) => Ok(self.push_literal(s, next_state_id)),
            Rule::Choice(elements) => {
                let mut alternative_state_ids = Vec::with_capacity(elements.len());
                for element in elements {
                    if self.expand_rule(element, next_state_id)? {
                        alternative_state_ids.push(self.nfa.last_state_id());
                    } else {
                        alternative_state_ids.push(next_state_id);
                    }
                }
                self.push_alternative_splits(alternative_state_ids);
                Ok(true)
            }
            Rule::Seq(elements) => {
                let mut result = false;
                for element in elements.iter().rev() {
                    if self.expand_rule(element, next_state_id)? {
                        result = true;
                    }
                    next_state_id = self.nfa.last_state_id();
                }
                Ok(result)
            }
            Rule::Repeat(rule) => {
                self.nfa.states.push(NfaState::Accept {
                    variable_index: 0,
                    precedence: 0,
                }); // Placeholder for split
                let split_state_id = self.nfa.last_state_id();
                if self.expand_rule(rule, split_state_id)? {
                    self.nfa.states[split_state_id as usize] =
                        NfaState::Split(self.nfa.last_state_id(), next_state_id);
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Rule::Metadata { rule, params } => {
                let has_precedence = if let Precedence::Integer(precedence) = &params.precedence {
                    self.precedence_stack.push(*precedence);
                    true
                } else {
                    false
                };
                let result = self.expand_rule(rule, next_state_id);
                if has_precedence {
                    self.precedence_stack.pop();
                }
                result
            }
            Rule::Blank => Ok(false),
            _ => Err(ExpandRuleError::UnexpectedRule(rule.clone()))?,
        }
    }

    fn expand_regex(&mut self, hir: &Hir, mut next_state_id: u32) -> ExpandRegexResult<bool> {
        match hir.kind() {
            HirKind::Empty => Ok(false),
            HirKind::Literal(literal) => {
                let literal = std::str::from_utf8(&literal.0)
                    .map_err(|e| ExpandRegexError::Utf8(e.to_string()))?;
                self.push_literal(literal, next_state_id);
                Ok(true)
            }
            HirKind::Class(class) => match class {
                Class::Unicode(class) => {
                    let mut chars = CharacterSet::default();
                    for c in class.ranges() {
                        chars = chars.add_range(c.start(), c.end());
                    }

                    // regex-syntax applies Unicode case folding for `s`, which includes
                    // the long s `ſ`. Tree-sitter's syntax expects `/s/i` to match only
                    // `s` and `S`, so remove that fold artifact when it is the only extra range.
                    if is_case_folded_long_s_set(&chars) {
                        chars = chars.difference(CharacterSet::from_char('ſ'));
                    }
                    self.push_advance(chars, next_state_id);
                    Ok(true)
                }
                Class::Bytes(bytes_class) => {
                    let mut chars = CharacterSet::default();
                    for c in bytes_class.ranges() {
                        chars = chars.add_range(c.start().into(), c.end().into());
                    }
                    self.push_advance(chars, next_state_id);
                    Ok(true)
                }
            },
            HirKind::Look(_) => Err(ExpandRegexError::Assertion)?,
            HirKind::Repetition(repetition) => match (repetition.min, repetition.max) {
                (0, Some(1)) => self.expand_zero_or_one(&repetition.sub, next_state_id),
                (1, None) => self.expand_one_or_more(&repetition.sub, next_state_id),
                (0, None) => self.expand_zero_or_more(&repetition.sub, next_state_id),
                (min, Some(max)) if min == max => {
                    self.expand_count(&repetition.sub, min, next_state_id)
                }
                (min, None) => {
                    if self.expand_zero_or_more(&repetition.sub, next_state_id)? {
                        self.expand_count(&repetition.sub, min, next_state_id)
                    } else {
                        Ok(false)
                    }
                }
                (min, Some(max)) => {
                    let mut result = self.expand_count(&repetition.sub, min, next_state_id)?;
                    for _ in min..max {
                        if result {
                            next_state_id = self.nfa.last_state_id();
                        }
                        if self.expand_zero_or_one(&repetition.sub, next_state_id)? {
                            result = true;
                        }
                    }
                    Ok(result)
                }
            },
            HirKind::Capture(capture) => self.expand_regex(&capture.sub, next_state_id),
            HirKind::Concat(concat) => {
                let mut result = false;
                for hir in concat.iter().rev() {
                    if self.expand_regex(hir, next_state_id)? {
                        result = true;
                        next_state_id = self.nfa.last_state_id();
                    }
                }
                Ok(result)
            }
            HirKind::Alternation(alternations) => {
                let mut alternative_state_ids = Vec::with_capacity(alternations.len());
                for hir in alternations {
                    if self.expand_regex(hir, next_state_id)? {
                        alternative_state_ids.push(self.nfa.last_state_id());
                    } else {
                        alternative_state_ids.push(next_state_id);
                    }
                }
                self.push_alternative_splits(alternative_state_ids);
                Ok(true)
            }
        }
    }

    fn expand_one_or_more(&mut self, hir: &Hir, next_state_id: u32) -> ExpandRegexResult<bool> {
        self.nfa.states.push(NfaState::Accept {
            variable_index: 0,
            precedence: 0,
        }); // Placeholder for split
        let split_state_id = self.nfa.last_state_id();
        if self.expand_regex(hir, split_state_id)? {
            self.nfa.states[split_state_id as usize] =
                NfaState::Split(self.nfa.last_state_id(), next_state_id);
            Ok(true)
        } else {
            self.nfa.states.pop();
            Ok(false)
        }
    }

    fn expand_zero_or_one(&mut self, hir: &Hir, next_state_id: u32) -> ExpandRegexResult<bool> {
        if self.expand_regex(hir, next_state_id)? {
            self.push_split(next_state_id);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn expand_zero_or_more(&mut self, hir: &Hir, next_state_id: u32) -> ExpandRegexResult<bool> {
        if self.expand_one_or_more(hir, next_state_id)? {
            self.push_split(next_state_id);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn expand_count(
        &mut self,
        hir: &Hir,
        count: u32,
        mut next_state_id: u32,
    ) -> ExpandRegexResult<bool> {
        let mut result = false;
        for _ in 0..count {
            if self.expand_regex(hir, next_state_id)? {
                result = true;
                next_state_id = self.nfa.last_state_id();
            }
        }
        Ok(result)
    }

    fn push_literal(&mut self, literal: &str, mut next_state_id: u32) -> bool {
        for character in literal.chars().rev() {
            self.push_advance(CharacterSet::from_char(character), next_state_id);
            next_state_id = self.nfa.last_state_id();
        }
        !literal.is_empty()
    }

    fn push_alternative_splits(&mut self, mut alternative_state_ids: Vec<u32>) {
        alternative_state_ids.sort_unstable();
        alternative_state_ids.dedup();
        alternative_state_ids.retain(|i| *i != self.nfa.last_state_id());
        for alternative_state_id in alternative_state_ids {
            self.push_split(alternative_state_id);
        }
    }

    fn push_advance(&mut self, chars: CharacterSet, state_id: u32) {
        let precedence = *self.precedence_stack.last().expect("precedence_stack is initialized with one element and is never emptied below its initial depth");
        self.nfa.states.push(NfaState::Advance {
            chars,
            state_id,
            precedence,
            is_sep: self.is_sep,
        });
    }

    fn push_split(&mut self, state_id: u32) {
        let last_state_id = self.nfa.last_state_id();
        self.nfa
            .states
            .push(NfaState::Split(state_id, last_state_id));
    }
}
