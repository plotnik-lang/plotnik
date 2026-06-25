use std::mem;

use rustc_hash::FxHashMap;

use super::super::{
    prepared::{ExtractedSyntaxGrammar, Variable, VariableType},
    rules::{Rule, Symbol},
};

pub(in crate::core::grammar) fn expand_repeats(
    mut grammar: ExtractedSyntaxGrammar,
) -> ExtractedSyntaxGrammar {
    let mut expander = Expander {
        variable_name: String::new(),
        repeat_count_in_variable: 0,
        preceding_symbol_count: grammar.variables.len(),
        auxiliary_variables: Vec::new(),
        existing_repeats: FxHashMap::default(),
    };

    for (i, variable) in grammar.variables.iter_mut().enumerate() {
        let expanded_top_level_repetition = expander.expand_variable(i, variable);

        // If a hidden variable had a top-level repetition and it was converted to
        // a recursive rule, then it can't be inlined.
        if expanded_top_level_repetition {
            grammar
                .variables_to_inline
                .retain(|symbol| *symbol != Symbol::non_terminal(i));
        }
    }

    grammar.variables.extend(expander.auxiliary_variables);
    grammar
}

struct Expander {
    variable_name: String,
    repeat_count_in_variable: usize,
    preceding_symbol_count: usize,
    auxiliary_variables: Vec<Variable>,
    existing_repeats: FxHashMap<Rule, Symbol>,
}

impl Expander {
    fn expand_variable(&mut self, index: usize, variable: &mut Variable) -> bool {
        self.variable_name.clear();
        self.variable_name.push_str(&variable.name);
        self.repeat_count_in_variable = 0;
        let mut rule = Rule::Blank;
        mem::swap(&mut rule, &mut variable.rule);

        // In the special case of a hidden variable with a repetition at its top level,
        // convert that rule itself into a binary tree structure instead of introducing
        // another auxiliary rule.
        if let (VariableType::Hidden, Rule::Repeat(repeated_content)) = (variable.kind, &rule) {
            let inner_rule = self.expand_rule(repeated_content);
            variable.rule = Self::wrap_rule_in_binary_tree(Symbol::non_terminal(index), inner_rule);
            variable.kind = VariableType::Auxiliary;
            return true;
        }

        variable.rule = self.expand_rule(&rule);
        false
    }

    fn expand_rule(&mut self, rule: &Rule) -> Rule {
        match rule {
            Rule::Choice(elements) => Rule::Choice(
                elements
                    .iter()
                    .map(|element| self.expand_rule(element))
                    .collect(),
            ),

            Rule::Seq(elements) => Rule::Seq(
                elements
                    .iter()
                    .map(|element| self.expand_rule(element))
                    .collect(),
            ),

            Rule::Metadata { rule, params } => Rule::Metadata {
                rule: Box::new(self.expand_rule(rule)),
                params: params.clone(),
            },

            // For repetitions, introduce an auxiliary rule that contains the
            // repeated content, but can also contain a recursive binary tree structure.
            Rule::Repeat(content) => {
                let inner_rule = self.expand_rule(content);

                if let Some(existing_symbol) = self.existing_repeats.get(&inner_rule) {
                    return Rule::Symbol(*existing_symbol);
                }

                self.repeat_count_in_variable += 1;
                let rule_name = format!(
                    "{}_repeat{}",
                    self.variable_name, self.repeat_count_in_variable
                );
                let repeat_symbol = Symbol::non_terminal(
                    self.preceding_symbol_count + self.auxiliary_variables.len(),
                );
                self.existing_repeats
                    .insert(inner_rule.clone(), repeat_symbol);
                self.auxiliary_variables.push(Variable::auxiliary(
                    rule_name,
                    Self::wrap_rule_in_binary_tree(repeat_symbol, inner_rule),
                ));

                Rule::Symbol(repeat_symbol)
            }

            _ => rule.clone(),
        }
    }

    fn wrap_rule_in_binary_tree(symbol: Symbol, rule: Rule) -> Rule {
        Rule::choice(vec![
            Rule::Seq(vec![Rule::Symbol(symbol), Rule::Symbol(symbol)]),
            rule,
        ])
    }
}
