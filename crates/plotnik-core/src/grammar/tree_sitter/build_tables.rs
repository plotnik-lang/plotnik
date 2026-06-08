mod build_parse_table;
mod coincident_tokens;
mod item;
pub(super) mod item_set_builder;
pub(super) mod token_conflicts;

use build_parse_table::BuildTableResult;
use log::debug;

use self::{
    build_parse_table::build_parse_table, coincident_tokens::CoincidentTokenIndex,
    item_set_builder::ParseItemSetBuilder, token_conflicts::TokenConflictMap,
};
use super::{
    grammars::{InlinedProductionMap, LexicalGrammar, SyntaxGrammar},
    nfa::NfaCursor,
    node_shapes::VariableInfo,
    rules::{Symbol, SymbolType, TokenSet},
    tables::{ParseAction, ParseTable, ParseTableEntry},
};

pub struct MetadataTables {
    pub parse_table: ParseTable,
}

pub fn build_metadata_tables(
    syntax_grammar: &SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
    variable_info: &[VariableInfo],
    inlines: &InlinedProductionMap,
) -> BuildTableResult<MetadataTables> {
    let item_set_builder = ParseItemSetBuilder::new(syntax_grammar, lexical_grammar, inlines);
    let following_tokens =
        get_following_tokens(syntax_grammar, lexical_grammar, inlines, &item_set_builder);
    let (mut parse_table, _) = build_parse_table(
        syntax_grammar,
        lexical_grammar,
        item_set_builder,
        variable_info,
    )?;
    let token_conflict_map = TokenConflictMap::new(lexical_grammar, following_tokens);
    let coincident_token_index = CoincidentTokenIndex::for_metadata(&parse_table, lexical_grammar);
    let keywords = identify_keywords(
        lexical_grammar,
        &parse_table,
        syntax_grammar.word_token,
        &token_conflict_map,
        &coincident_token_index,
    );
    populate_error_state(
        &mut parse_table,
        syntax_grammar,
        lexical_grammar,
        &coincident_token_index,
        &token_conflict_map,
        &keywords,
    );
    populate_used_symbols(&mut parse_table, syntax_grammar, lexical_grammar);

    Ok(MetadataTables { parse_table })
}

pub(super) fn get_following_tokens(
    syntax_grammar: &SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
    inlines: &InlinedProductionMap,
    builder: &ParseItemSetBuilder,
) -> Vec<TokenSet> {
    let n_terminals = lexical_grammar.variables.len();
    let n_externals = syntax_grammar.external_tokens.len();
    let mut result = vec![TokenSet::with_capacity(n_terminals, n_externals); n_terminals];
    let productions = syntax_grammar
        .variables
        .iter()
        .flat_map(|v| &v.productions)
        .chain(&inlines.productions);
    let all_tokens = (0..result.len())
        .map(Symbol::terminal)
        .collect::<TokenSet>();
    for production in productions {
        for i in 1..production.steps.len() {
            let left_tokens = builder.last_set(&production.steps[i - 1].symbol);
            let right_tokens = builder.first_set(&production.steps[i].symbol);
            let right_reserved_tokens = builder.reserved_first_set(&production.steps[i].symbol);
            for left_token in left_tokens.iter() {
                if left_token.is_terminal() {
                    result[left_token.index].insert_all_terminals(right_tokens);
                    if let Some(reserved_tokens) = right_reserved_tokens {
                        result[left_token.index].insert_all_terminals(reserved_tokens);
                    }
                }
            }
        }
    }
    for extra in &syntax_grammar.extra_symbols {
        if extra.is_terminal() {
            for entry in &mut result {
                entry.insert(*extra);
            }
            result[extra.index] = all_tokens.clone();
        }
    }
    result
}

fn populate_error_state(
    parse_table: &mut ParseTable,
    syntax_grammar: &SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
    coincident_token_index: &CoincidentTokenIndex,
    token_conflict_map: &TokenConflictMap,
    keywords: &TokenSet,
) {
    let state = &mut parse_table.states[0];
    let n = lexical_grammar.variables.len();

    // First identify the *conflict-free tokens*: tokens that do not overlap with
    // any other token in any way, besides matching exactly the same string.
    let conflict_free_tokens = (0..n)
        .filter_map(|i| {
            let conflicts_with_other_tokens = (0..n).any(|j| {
                j != i
                    && !coincident_token_index.contains(Symbol::terminal(i), Symbol::terminal(j))
                    && token_conflict_map.does_match_shorter_or_longer(i, j)
            });
            if conflicts_with_other_tokens {
                None
            } else {
                debug!(
                    "error recovery - token {} has no conflicts",
                    lexical_grammar.variables[i].name
                );
                Some(Symbol::terminal(i))
            }
        })
        .collect::<TokenSet>();

    let recover_entry = ParseTableEntry {
        reusable: false,
        actions: vec![ParseAction::Recover],
    };

    // Exclude from the error-recovery state any token that conflicts with one of
    // the *conflict-free tokens* identified above.
    for i in 0..n {
        let symbol = Symbol::terminal(i);
        if !conflict_free_tokens.contains(&symbol)
            && !keywords.contains(&symbol)
            && syntax_grammar.word_token != Some(symbol)
            && let Some(t) = conflict_free_tokens.iter().find(|t| {
                !coincident_token_index.contains(symbol, *t)
                    && token_conflict_map.does_conflict(symbol.index, t.index)
            })
        {
            debug!(
                "error recovery - exclude token {} because of conflict with {}",
                lexical_grammar.variables[i].name, lexical_grammar.variables[t.index].name
            );
            continue;
        }
        debug!(
            "error recovery - include token {}",
            lexical_grammar.variables[i].name
        );
        state
            .terminal_entries
            .entry(symbol)
            .or_insert_with(|| recover_entry.clone());
    }

    for (i, external_token) in syntax_grammar.external_tokens.iter().enumerate() {
        if external_token.corresponding_internal_token.is_none() {
            state
                .terminal_entries
                .entry(Symbol::external(i))
                .or_insert_with(|| recover_entry.clone());
        }
    }

    state.terminal_entries.insert(Symbol::end(), recover_entry);
}

fn populate_used_symbols(
    parse_table: &mut ParseTable,
    syntax_grammar: &SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
) {
    let mut terminal_usages = vec![false; lexical_grammar.variables.len()];
    let mut non_terminal_usages = vec![false; syntax_grammar.variables.len()];
    let mut external_usages = vec![false; syntax_grammar.external_tokens.len()];
    for state in &parse_table.states {
        for symbol in state.terminal_entries.keys() {
            match symbol.kind {
                SymbolType::Terminal => terminal_usages[symbol.index] = true,
                SymbolType::External => external_usages[symbol.index] = true,
                _ => {}
            }
        }
        for symbol in state.nonterminal_entries.keys() {
            non_terminal_usages[symbol.index] = true;
        }
    }
    parse_table.symbols.push(Symbol::end());
    for (i, value) in terminal_usages.into_iter().enumerate() {
        if value {
            // Assign the grammar's word token a low numerical index. This ensures that
            // it can be stored in a subtree with no heap allocations, even for grammars with
            // very large numbers of tokens. This is an optimization, but it's also important to
            // ensure that a subtree's symbol can be successfully reassigned to the word token
            // without having to move the subtree to the heap.
            // See https://github.com/tree-sitter/tree-sitter/issues/258
            if syntax_grammar.word_token.is_some_and(|t| t.index == i) {
                parse_table.symbols.insert(1, Symbol::terminal(i));
            } else {
                parse_table.symbols.push(Symbol::terminal(i));
            }
        }
    }
    for (i, value) in external_usages.into_iter().enumerate() {
        if value {
            parse_table.symbols.push(Symbol::external(i));
        }
    }
    for (i, value) in non_terminal_usages.into_iter().enumerate() {
        if value {
            parse_table.symbols.push(Symbol::non_terminal(i));
        }
    }
}

fn identify_keywords(
    lexical_grammar: &LexicalGrammar,
    parse_table: &ParseTable,
    word_token: Option<Symbol>,
    token_conflict_map: &TokenConflictMap,
    coincident_token_index: &CoincidentTokenIndex,
) -> TokenSet {
    if word_token.is_none() {
        return TokenSet::new();
    }

    let word_token = word_token.unwrap();
    let mut cursor = NfaCursor::new(&lexical_grammar.nfa, Vec::new());

    // First find all of the candidate keyword tokens: tokens that start with
    // letters or underscore and can match the same string as a word token.
    let keyword_candidates = lexical_grammar
        .variables
        .iter()
        .enumerate()
        .filter_map(|(i, variable)| {
            cursor.reset(vec![variable.start_state]);
            if all_chars_are_alphabetical(&cursor)
                && token_conflict_map.does_match_same_string(i, word_token.index)
                && !token_conflict_map.does_match_different_string(i, word_token.index)
            {
                debug!(
                    "Keywords - add candidate {}",
                    lexical_grammar.variables[i].name
                );
                Some(Symbol::terminal(i))
            } else {
                None
            }
        })
        .collect::<TokenSet>();

    // Exclude keyword candidates that shadow another keyword candidate.
    let keywords = keyword_candidates
        .iter()
        .filter(|token| {
            for other_token in keyword_candidates.iter() {
                if other_token != *token
                    && token_conflict_map.does_match_same_string(other_token.index, token.index)
                {
                    debug!(
                        "Keywords - exclude {} because it matches the same string as {}",
                        lexical_grammar.variables[token.index].name,
                        lexical_grammar.variables[other_token.index].name
                    );
                    return false;
                }
            }
            true
        })
        .collect::<TokenSet>();

    // Exclude keyword candidates for which substituting the keyword capture
    // token would introduce new lexical conflicts with other tokens.

    keywords
        .iter()
        .filter(|token| {
            for other_index in 0..lexical_grammar.variables.len() {
                if keyword_candidates.contains(&Symbol::terminal(other_index)) {
                    continue;
                }

                // If the word token was already valid in every state containing
                // this keyword candidate, then substituting the word token won't
                // introduce any new lexical conflicts.
                if coincident_token_index
                    .states_with(*token, Symbol::terminal(other_index))
                    .iter()
                    .all(|state_id| {
                        parse_table.states[*state_id]
                            .terminal_entries
                            .contains_key(&word_token)
                    })
                {
                    continue;
                }

                if !token_conflict_map.has_same_conflict_status(
                    token.index,
                    word_token.index,
                    other_index,
                ) {
                    debug!(
                        "Keywords - exclude {} because of conflict with {}",
                        lexical_grammar.variables[token.index].name,
                        lexical_grammar.variables[other_index].name
                    );
                    return false;
                }
            }

            debug!(
                "Keywords - include {}",
                lexical_grammar.variables[token.index].name,
            );
            true
        })
        .collect()
}

fn all_chars_are_alphabetical(cursor: &NfaCursor) -> bool {
    cursor.transition_chars().all(|(chars, is_sep)| {
        if is_sep {
            true
        } else {
            chars.chars().all(|c| c.is_alphabetic() || c == '_')
        }
    })
}
