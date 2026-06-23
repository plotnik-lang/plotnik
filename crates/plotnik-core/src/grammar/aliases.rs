use std::cmp::Reverse;

use super::{
    prepared::{LexicalGrammar, SyntaxGrammar},
    rules::{Alias, AliasMap, Symbol, SymbolType},
};

#[derive(Clone, Default)]
struct AliasUsage {
    aliases: Vec<(Alias, usize)>,
    appears_unaliased: bool,
}

struct AliasUsageTable {
    terminals: Vec<AliasUsage>,
    non_terminals: Vec<AliasUsage>,
    externals: Vec<AliasUsage>,
}

impl AliasUsageTable {
    fn new(syntax_grammar: &SyntaxGrammar, lexical_grammar: &LexicalGrammar) -> Self {
        Self {
            terminals: vec![AliasUsage::default(); lexical_grammar.variables.len()],
            non_terminals: vec![AliasUsage::default(); syntax_grammar.variables.len()],
            externals: vec![AliasUsage::default(); syntax_grammar.external_tokens.len()],
        }
    }

    fn get_mut(&mut self, symbol: Symbol) -> &mut AliasUsage {
        match symbol.kind {
            SymbolType::External => &mut self.externals[symbol.index],
            SymbolType::NonTerminal => &mut self.non_terminals[symbol.index],
            SymbolType::Terminal => &mut self.terminals[symbol.index],
            SymbolType::End | SymbolType::EndOfNonTerminalExtra => panic!("Unexpected end token"),
        }
    }

    fn for_each_mut(&mut self, mut visit: impl FnMut(Symbol, &mut AliasUsage)) {
        for (i, status) in self.terminals.iter_mut().enumerate() {
            visit(Symbol::terminal(i), status);
        }
        for (i, status) in self.non_terminals.iter_mut().enumerate() {
            visit(Symbol::non_terminal(i), status);
        }
        for (i, status) in self.externals.iter_mut().enumerate() {
            visit(Symbol::external(i), status);
        }
    }
}

// Promotes a "default alias" for any symbol that appears exclusively under aliases.
// Two reasons: (1) avoids storing per-production alias info in the parse table, and
// (2) `ERROR` nodes skip context-specific aliases, so without a default alias those
// children would have no alias at all — a visible inconsistency.
pub(super) fn extract_default_aliases(
    syntax_grammar: &mut SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
) -> AliasMap {
    let mut statuses = AliasUsageTable::new(syntax_grammar, lexical_grammar);

    for variable in &syntax_grammar.variables {
        for production in &variable.productions {
            for step in &production.steps {
                let status = statuses.get_mut(step.symbol);

                // Default aliases don't work for inlined variables.
                if syntax_grammar.variables_to_inline.contains(&step.symbol) {
                    continue;
                }

                if let Some(alias) = &step.alias {
                    if let Some(count_for_alias) = status
                        .aliases
                        .iter_mut()
                        .find_map(|(a, count)| if a == alias { Some(count) } else { None })
                    {
                        *count_for_alias += 1;
                    } else {
                        status.aliases.push((alias.clone(), 1));
                    }
                } else {
                    status.appears_unaliased = true;
                }
            }
        }
    }

    for symbol in &syntax_grammar.extra_symbols {
        statuses.get_mut(*symbol).appears_unaliased = true;
    }

    let mut result = AliasMap::new();
    statuses.for_each_mut(|symbol, status| {
        if status.appears_unaliased {
            status.aliases.clear();
            return;
        }

        // On ties, keep the earliest alias so selection is deterministic across grammars.
        if let Some((_, default_entry)) = status
            .aliases
            .drain(..)
            .enumerate()
            .max_by_key(|(i, (_, count))| (*count, Reverse(*i)))
        {
            status
                .aliases
                .push((default_entry.0.clone(), default_entry.1));
            result.insert(symbol, default_entry.0);
        }
    });

    // The default alias is now implicit, so remove the per-step alias where it matches.
    let mut alias_positions_to_clear = Vec::new();
    for variable in &mut syntax_grammar.variables {
        alias_positions_to_clear.clear();

        for (i, production) in variable.productions.iter().enumerate() {
            for (j, step) in production.steps.iter().enumerate() {
                let status = statuses.get_mut(step.symbol);

                // If this step is aliased as the symbol's default alias, then remove that alias.
                if step.alias.is_some()
                    && step.alias.as_ref() == status.aliases.first().map(|t| &t.0)
                {
                    let mut other_productions_must_use_this_alias_at_this_index = false;
                    for (other_i, other_production) in variable.productions.iter().enumerate() {
                        if other_i != i
                            && other_production.steps.len() > j
                            && other_production.steps[j].alias == step.alias
                            && result.get(&other_production.steps[j].symbol) != step.alias.as_ref()
                        {
                            other_productions_must_use_this_alias_at_this_index = true;
                            break;
                        }
                    }

                    if !other_productions_must_use_this_alias_at_this_index {
                        alias_positions_to_clear.push((i, j));
                    }
                }
            }
        }

        for (production_index, step_index) in &alias_positions_to_clear {
            variable.productions[*production_index].steps[*step_index].alias = None;
        }
    }

    result
}
