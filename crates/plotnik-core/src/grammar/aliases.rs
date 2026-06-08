use std::cmp::Reverse;

use super::{
    prepared::{LexicalGrammar, SyntaxGrammar},
    rules::{Alias, AliasMap, Symbol, SymbolType},
};

#[derive(Clone, Default)]
struct SymbolStatus {
    aliases: Vec<(Alias, usize)>,
    appears_unaliased: bool,
}

struct SymbolStatuses {
    terminals: Vec<SymbolStatus>,
    non_terminals: Vec<SymbolStatus>,
    externals: Vec<SymbolStatus>,
}

impl SymbolStatuses {
    fn new(syntax_grammar: &SyntaxGrammar, lexical_grammar: &LexicalGrammar) -> Self {
        Self {
            terminals: vec![SymbolStatus::default(); lexical_grammar.variables.len()],
            non_terminals: vec![SymbolStatus::default(); syntax_grammar.variables.len()],
            externals: vec![SymbolStatus::default(); syntax_grammar.external_tokens.len()],
        }
    }

    fn get_mut(&mut self, symbol: Symbol) -> &mut SymbolStatus {
        match symbol.kind {
            SymbolType::External => &mut self.externals[symbol.index],
            SymbolType::NonTerminal => &mut self.non_terminals[symbol.index],
            SymbolType::Terminal => &mut self.terminals[symbol.index],
            SymbolType::End | SymbolType::EndOfNonTerminalExtra => panic!("Unexpected end token"),
        }
    }

    fn for_each_mut(&mut self, mut visit: impl FnMut(Symbol, &mut SymbolStatus)) {
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

// Update the grammar by finding symbols that always are aliased, and for each such symbol,
// promoting one of its aliases to a "default alias", which is applied globally instead
// of in a context-specific way.
//
// This has two benefits:
// * It reduces the overhead of storing production-specific alias info in the parse table.
// * Within an `ERROR` node, no context-specific aliases will be applied. This transformation
//   ensures that the children of an `ERROR` node have symbols that are consistent with the way that
//   they would appear in a valid syntax tree.
pub(super) fn extract_default_aliases(
    syntax_grammar: &mut SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
) -> AliasMap {
    let mut statuses = SymbolStatuses::new(syntax_grammar, lexical_grammar);

    // For each grammar symbol, find all of the aliases under which the symbol appears,
    // and determine whether or not the symbol ever appears *unaliased*.
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

    // For each symbol that always appears aliased, find the alias that occurs most often,
    // and designate that alias as the symbol's "default alias". Store all of these
    // default aliases in a map that will be returned.
    let mut result = AliasMap::new();
    statuses.for_each_mut(|symbol, status| {
        if status.appears_unaliased {
            status.aliases.clear();
            return;
        }

        // Prefer the highest alias count; on ties, keep the earliest discovered alias
        // so default alias selection stays stable.
        if let Some(default_entry) = status
            .aliases
            .iter()
            .enumerate()
            .max_by_key(|(i, (_, count))| (*count, Reverse(*i)))
            .map(|(_, entry)| entry.clone())
        {
            status.aliases.clear();
            status.aliases.push(default_entry.clone());
            result.insert(symbol, default_entry.0);
        }
    });

    // Wherever a symbol is aliased as its default alias, remove the usage of the alias,
    // because it will now be redundant.
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
