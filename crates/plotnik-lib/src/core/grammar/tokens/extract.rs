use rustc_hash::FxHashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::super::{
    prepared::{
        ExternalToken, ExtractedLexicalGrammar, ExtractedSyntaxGrammar, InternedGrammar,
        ReservedWordSet, Variable, VariableType,
    },
    rules::{MetadataParams, Rule, Symbol, SymbolType},
};

pub type ExtractTokensResult<T> = Result<T, ExtractTokensError>;

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum ExtractTokensError {
    #[error(
        "The rule `{0}` contains an empty string.

Tree-sitter does not support syntactic rules that contain an empty string
unless they are used only as the grammar's start rule.
"
    )]
    EmptyString(String),
    #[error("Terminal rule '{0}' cannot be used as a supertype")]
    SupertypeTerminal(String),
    #[error("Rule '{0}' cannot be used as both an external token and a non-terminal rule")]
    ExternalTokenNonTerminal(String),
    #[error("Non-symbol rules cannot be used as external tokens")]
    NonSymbolExternalToken,
    #[error(transparent)]
    WordToken(NonTerminalWordTokenError),
    #[error("Reserved word '{0}' must be a token")]
    NonTokenReservedWord(String),
}

#[derive(Debug, Error, Serialize, Deserialize)]
pub struct NonTerminalWordTokenError {
    pub symbol_name: String,
    pub conflicting_symbol_name: Option<String>,
}

impl std::fmt::Display for NonTerminalWordTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Non-terminal symbol '{}' cannot be used as the word token",
            self.symbol_name
        )?;
        if let Some(conflicting_name) = &self.conflicting_symbol_name {
            writeln!(
                f,
                ", because its rule is duplicated in '{conflicting_name}'",
            )
        } else {
            writeln!(f)
        }
    }
}

pub(in crate::core::grammar) fn extract_tokens(
    mut grammar: InternedGrammar,
) -> ExtractTokensResult<(ExtractedSyntaxGrammar, ExtractedLexicalGrammar)> {
    let mut extractor = TokenExtractor {
        current_variable_name: String::new(),
        current_variable_token_count: 0,
        is_first_rule: false,
        extracted_variables: Vec::new(),
        extracted_usage_counts: Vec::new(),
    };

    for (i, variable) in &mut grammar.variables.iter_mut().enumerate() {
        extractor.extract_tokens_in_variable(i == 0, variable)?;
    }

    for variable in &mut grammar.external_tokens {
        extractor.extract_tokens_in_variable(false, variable)?;
    }

    let mut lexical_variables = extractor.extracted_variables;

    let (mut variables, mut symbol_replacer) = promote_extracted_tokens(
        grammar.variables,
        &mut lexical_variables,
        &extractor.extracted_usage_counts,
    );

    for variable in &mut variables {
        let rule = std::mem::take(&mut variable.rule);
        variable.rule = symbol_replacer.replace_symbols_in_rule(rule);
    }

    let supertype_symbols: Vec<Symbol> = grammar
        .supertype_symbols
        .into_iter()
        .map(|symbol| symbol_replacer.replace_symbol(symbol))
        .collect();
    for supertype_symbol in &supertype_symbols {
        if supertype_symbol.is_terminal() {
            Err(ExtractTokensError::SupertypeTerminal(
                lexical_variables[supertype_symbol.index].name.clone(),
            ))?;
        }
    }

    let variables_to_inline = grammar
        .variables_to_inline
        .into_iter()
        .map(|symbol| symbol_replacer.replace_symbol(symbol))
        .collect();

    let mut separators = Vec::new();
    let mut extra_symbols = Vec::new();
    for rule in grammar.extra_symbols {
        if let Some(symbol) =
            extracted_token_symbol_for_rule(&rule, &lexical_variables, &symbol_replacer)
        {
            extra_symbols.push(symbol);
        } else {
            separators.push(rule);
        }
    }

    let mut external_tokens = Vec::with_capacity(grammar.external_tokens.len());
    for external_token in grammar.external_tokens {
        let rule = symbol_replacer.replace_symbols_in_rule(external_token.rule);
        if let Rule::Symbol(symbol) = rule {
            if symbol.is_non_terminal() {
                Err(ExtractTokensError::ExternalTokenNonTerminal(
                    variables[symbol.index].name.clone(),
                ))?;
            }

            if symbol.is_external() {
                external_tokens.push(ExternalToken::external(
                    external_token.name,
                    external_token.kind,
                ));
            } else {
                external_tokens.push(ExternalToken::internal(
                    lexical_variables[symbol.index].name.clone(),
                    external_token.kind,
                    symbol,
                ));
            }
        } else {
            Err(ExtractTokensError::NonSymbolExternalToken)?;
        }
    }

    let word_token = if let Some(token) = grammar.word_token {
        let token = symbol_replacer.replace_symbol(token);
        if token.is_non_terminal() {
            let word_token_variable = &variables[token.index];
            let conflicting_symbol_name = variables
                .iter()
                .enumerate()
                .find(|(i, v)| *i != token.index && v.rule == word_token_variable.rule)
                .map(|(_, v)| v.name.clone());

            Err(ExtractTokensError::WordToken(NonTerminalWordTokenError {
                symbol_name: word_token_variable.name.clone(),
                conflicting_symbol_name,
            }))?;
        }
        Some(token)
    } else {
        None
    };

    let mut reserved_word_contexts = Vec::with_capacity(grammar.reserved_word_sets.len());
    for reserved_word_context in grammar.reserved_word_sets {
        let mut reserved_words = Vec::with_capacity(reserved_word_contexts.len());
        for reserved_rule in reserved_word_context.reserved_words {
            if let Some(symbol) = extracted_token_symbol_for_rule(
                &reserved_rule,
                &lexical_variables,
                &symbol_replacer,
            ) {
                reserved_words.push(symbol);
            } else {
                let rule = if let Rule::Metadata { rule, .. } = &reserved_rule {
                    rule.as_ref()
                } else {
                    &reserved_rule
                };
                let token_name = match rule {
                    Rule::String(s) => s.clone(),
                    Rule::Pattern(p, _) => p.clone(),
                    _ => "unknown".to_string(),
                };
                Err(ExtractTokensError::NonTokenReservedWord(token_name))?;
            }
        }
        reserved_word_contexts.push(ReservedWordSet {
            name: reserved_word_context.name,
            reserved_words,
        });
    }

    Ok((
        ExtractedSyntaxGrammar {
            variables,
            extra_symbols,
            external_tokens,
            variables_to_inline,
            supertype_symbols,
            word_token,
            reserved_word_sets: reserved_word_contexts,
        },
        ExtractedLexicalGrammar {
            variables: lexical_variables,
            separators,
        },
    ))
}

struct TokenExtractor {
    current_variable_name: String,
    current_variable_token_count: usize,
    is_first_rule: bool,
    extracted_variables: Vec<Variable>,
    extracted_usage_counts: Vec<usize>,
}

struct SymbolReplacer {
    replacements: FxHashMap<usize, usize>,
}

fn promote_extracted_tokens(
    variables: Vec<Variable>,
    lexical_variables: &mut [Variable],
    extracted_usage_counts: &[usize],
) -> (Vec<Variable>, SymbolReplacer) {
    // If a variable's entire rule was extracted as a token and that token didn't
    // appear within any other rule, then remove that variable from the syntax
    // grammar, giving its name to the token in the lexical grammar. Any symbols
    // that pointed to that variable will need to be updated to point to the
    // variable in the lexical grammar. Symbols that pointed to later variables
    // will need to have their indices decremented.
    let mut retained_variables = Vec::with_capacity(variables.len());
    let mut symbol_replacer = SymbolReplacer {
        replacements: FxHashMap::default(),
    };

    for (i, variable) in variables.into_iter().enumerate() {
        if let Rule::Symbol(Symbol {
            kind: SymbolType::Terminal,
            index,
        }) = variable.rule
            && i > 0
            && extracted_usage_counts[index] == 1
        {
            let lexical_variable = &mut lexical_variables[index];
            if lexical_variable.kind == VariableType::Auxiliary
                || variable.kind != VariableType::Hidden
            {
                lexical_variable.kind = variable.kind;
                lexical_variable.name = variable.name;
                symbol_replacer.replacements.insert(i, index);
                continue;
            }
        }
        retained_variables.push(variable);
    }

    (retained_variables, symbol_replacer)
}

fn extracted_token_symbol_for_rule(
    rule: &Rule,
    lexical_variables: &[Variable],
    symbol_replacer: &SymbolReplacer,
) -> Option<Symbol> {
    if let Rule::Symbol(symbol) = rule {
        return Some(symbol_replacer.replace_symbol(*symbol));
    }

    lexical_variables
        .iter()
        .position(|variable| variable.rule == *rule)
        .map(Symbol::terminal)
}

impl TokenExtractor {
    fn extract_tokens_in_variable(
        &mut self,
        is_first: bool,
        variable: &mut Variable,
    ) -> ExtractTokensResult<()> {
        self.current_variable_name.clear();
        self.current_variable_name.push_str(&variable.name);
        self.current_variable_token_count = 0;
        self.is_first_rule = is_first;
        let rule = std::mem::take(&mut variable.rule);
        variable.rule = self.extract_tokens_in_rule(rule)?;
        Ok(())
    }

    fn extract_tokens_in_rule(&mut self, input: Rule) -> ExtractTokensResult<Rule> {
        match input {
            Rule::String(name) => {
                let rule = Rule::String(name.clone());
                Ok(self.extract_token(rule, Some(name))?.into())
            }
            Rule::Pattern(..) => Ok(self.extract_token(input, None)?.into()),
            Rule::Metadata { params, rule } => self.extract_metadata_rule(params, *rule),
            Rule::Repeat(content) => Ok(Rule::Repeat(Box::new(
                self.extract_tokens_in_rule(*content)?,
            ))),
            Rule::Seq(elements) => Ok(Rule::Seq(
                elements
                    .into_iter()
                    .map(|e| self.extract_tokens_in_rule(e))
                    .collect::<ExtractTokensResult<Vec<_>>>()?,
            )),
            Rule::Choice(elements) => Ok(Rule::Choice(
                elements
                    .into_iter()
                    .map(|e| self.extract_tokens_in_rule(e))
                    .collect::<ExtractTokensResult<Vec<_>>>()?,
            )),
            Rule::Reserved { rule, context_name } => Ok(Rule::Reserved {
                rule: Box::new(self.extract_tokens_in_rule(*rule)?),
                context_name,
            }),
            _ => Ok(input),
        }
    }

    fn extract_metadata_rule(
        &mut self,
        params: MetadataParams,
        rule: Rule,
    ) -> ExtractTokensResult<Rule> {
        if params.is_token {
            let mut cleared_params = params.clone();
            cleared_params.is_token = false;

            let string_value = if let Rule::String(value) = &rule {
                Some(value.clone())
            } else {
                None
            };

            let rule_to_extract = if cleared_params == MetadataParams::default() {
                rule
            } else {
                Rule::Metadata {
                    params,
                    rule: Box::new(rule),
                }
            };

            return Ok(self.extract_token(rule_to_extract, string_value)?.into());
        }

        Ok(Rule::Metadata {
            params,
            rule: Box::new(self.extract_tokens_in_rule(rule)?),
        })
    }

    fn extract_token(
        &mut self,
        rule: Rule,
        string_value: Option<String>,
    ) -> ExtractTokensResult<Symbol> {
        for (i, variable) in self.extracted_variables.iter_mut().enumerate() {
            if variable.rule == rule {
                self.extracted_usage_counts[i] += 1;
                return Ok(Symbol::terminal(i));
            }
        }

        let index = self.extracted_variables.len();
        let variable = if let Some(string_value) = string_value {
            if string_value.is_empty() && !self.is_first_rule {
                Err(ExtractTokensError::EmptyString(
                    self.current_variable_name.clone(),
                ))?;
            }
            Variable::anonymous(string_value, rule)
        } else {
            self.current_variable_token_count += 1;
            Variable::auxiliary(
                format!(
                    "{}_token{}",
                    self.current_variable_name, self.current_variable_token_count
                ),
                rule,
            )
        };

        self.extracted_variables.push(variable);
        self.extracted_usage_counts.push(1);
        Ok(Symbol::terminal(index))
    }
}

impl SymbolReplacer {
    fn replace_symbols_in_rule(&mut self, rule: Rule) -> Rule {
        match rule {
            Rule::Symbol(symbol) => self.replace_symbol(symbol).into(),
            Rule::Choice(elements) => Rule::Choice(
                elements
                    .into_iter()
                    .map(|e| self.replace_symbols_in_rule(e))
                    .collect(),
            ),
            Rule::Seq(elements) => Rule::Seq(
                elements
                    .into_iter()
                    .map(|e| self.replace_symbols_in_rule(e))
                    .collect(),
            ),
            Rule::Repeat(content) => Rule::Repeat(Box::new(self.replace_symbols_in_rule(*content))),
            Rule::Metadata { rule, params } => Rule::Metadata {
                params,
                rule: Box::new(self.replace_symbols_in_rule(*rule)),
            },
            Rule::Reserved { rule, context_name } => Rule::Reserved {
                rule: Box::new(self.replace_symbols_in_rule(*rule)),
                context_name,
            },
            _ => rule,
        }
    }

    fn replace_symbol(&self, symbol: Symbol) -> Symbol {
        if !symbol.is_non_terminal() {
            return symbol;
        }

        if let Some(replacement) = self.replacements.get(&symbol.index) {
            return Symbol::terminal(*replacement);
        }

        let mut adjusted_index = symbol.index;
        for replaced_index in self.replacements.keys() {
            if *replaced_index < symbol.index {
                adjusted_index -= 1;
            }
        }

        Symbol::non_terminal(adjusted_index)
    }
}
