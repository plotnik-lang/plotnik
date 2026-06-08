use rustc_hash::FxHashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::super::{
    prepared::{InlinedProductionMap, LexicalGrammar, Production, ProductionStep, SyntaxGrammar},
    rules::SymbolType,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ProductionStepId {
    // A `None` value here means that the production itself was produced via inlining,
    // and is stored in the builder's `productions` vector, as opposed to being
    // stored in one of the grammar's variables.
    variable: Option<usize>,
    production: usize,
    step: usize,
}

struct InlinedProductionMapBuilder {
    production_indices_by_step_id: FxHashMap<ProductionStepId, Vec<usize>>,
    productions: Vec<Production>,
}

impl InlinedProductionMapBuilder {
    fn build(mut self, grammar: &SyntaxGrammar) -> InlinedProductionMap {
        let mut step_ids_to_process = Vec::new();
        for (variable_index, variable) in grammar.variables.iter().enumerate() {
            for production_index in 0..variable.productions.len() {
                step_ids_to_process.push(ProductionStepId {
                    variable: Some(variable_index),
                    production: production_index,
                    step: 0,
                });
                while !step_ids_to_process.is_empty() {
                    let mut i = 0;
                    while i < step_ids_to_process.len() {
                        let step_id = step_ids_to_process[i];
                        if let Some(step) = self.production_step_for_id(step_id, grammar) {
                            if grammar.variables_to_inline.contains(&step.symbol) {
                                let inlined_step_ids = self
                                    .inline_production_at_step(step_id, grammar)
                                    .iter()
                                    .copied()
                                    .map(|production_index| ProductionStepId {
                                        variable: None,
                                        production: production_index,
                                        step: step_id.step,
                                    });
                                step_ids_to_process.splice(i..=i, inlined_step_ids);
                            } else {
                                step_ids_to_process[i] = ProductionStepId {
                                    variable: step_id.variable,
                                    production: step_id.production,
                                    step: step_id.step + 1,
                                };
                                i += 1;
                            }
                        } else {
                            step_ids_to_process.remove(i);
                        }
                    }
                }
            }
        }

        InlinedProductionMap {
            productions: self.productions,
        }
    }

    fn inline_production_at_step<'a>(
        &'a mut self,
        step_id: ProductionStepId,
        grammar: &'a SyntaxGrammar,
    ) -> &'a [usize] {
        // Build a list of productions produced by inlining rules.
        let mut i = 0;
        let step_index = step_id.step;
        let mut productions_to_add = vec![self.production_for_id(step_id, grammar).clone()];
        while i < productions_to_add.len() {
            if let Some(step) = productions_to_add[i].steps.get(step_index) {
                let symbol = step.symbol;
                if grammar.variables_to_inline.contains(&symbol) {
                    // Remove the production from the vector, replacing it with a placeholder.
                    let production = productions_to_add
                        .splice(i..=i, std::iter::once(&Production::default()).cloned())
                        .next()
                        .unwrap();

                    // Replace the placeholder with the inlined productions.
                    productions_to_add.splice(
                        i..=i,
                        grammar.variables[symbol.index].productions.iter().map(|p| {
                            let mut production = production.clone();
                            let removed_step = production
                                .steps
                                .splice(step_index..=step_index, p.steps.iter().cloned())
                                .next()
                                .unwrap();
                            let inserted_steps =
                                &mut production.steps[step_index..(step_index + p.steps.len())];
                            if let Some(alias) = removed_step.alias {
                                for inserted_step in inserted_steps.iter_mut() {
                                    inserted_step.alias = Some(alias.clone());
                                }
                            }
                            if let Some(field_name) = removed_step.field_name {
                                for inserted_step in inserted_steps.iter_mut() {
                                    inserted_step.field_name = Some(field_name.clone());
                                }
                            }
                            if let Some(last_inserted_step) = inserted_steps.last_mut() {
                                if last_inserted_step.precedence.is_none() {
                                    last_inserted_step.precedence = removed_step.precedence;
                                }
                                if last_inserted_step.associativity.is_none() {
                                    last_inserted_step.associativity = removed_step.associativity;
                                }
                            }
                            if p.dynamic_precedence.abs() > production.dynamic_precedence.abs() {
                                production.dynamic_precedence = p.dynamic_precedence;
                            }
                            production
                        }),
                    );

                    continue;
                }
            }
            i += 1;
        }

        // Store all the computed productions.
        let result = productions_to_add
            .into_iter()
            .map(|production| {
                self.productions
                    .iter()
                    .position(|p| *p == production)
                    .unwrap_or_else(|| {
                        self.productions.push(production);
                        self.productions.len() - 1
                    })
            })
            .collect();

        // Cache these productions based on the original production step.
        self.production_indices_by_step_id
            .entry(step_id)
            .or_insert(result)
    }

    fn production_for_id<'a>(
        &'a self,
        id: ProductionStepId,
        grammar: &'a SyntaxGrammar,
    ) -> &'a Production {
        id.variable.map_or_else(
            || &self.productions[id.production],
            |variable_index| &grammar.variables[variable_index].productions[id.production],
        )
    }

    fn production_step_for_id<'a>(
        &'a self,
        id: ProductionStepId,
        grammar: &'a SyntaxGrammar,
    ) -> Option<&'a ProductionStep> {
        self.production_for_id(id, grammar).steps.get(id.step)
    }
}

pub type ProcessInlinesResult<T> = Result<T, ProcessInlinesError>;

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum ProcessInlinesError {
    #[error("External token `{0}` cannot be inlined")]
    ExternalToken(String),
    #[error("Token `{0}` cannot be inlined")]
    Token(String),
    #[error("Rule `{0}` cannot be inlined because it is the first rule")]
    FirstRule(String),
}

pub(in crate::grammar) fn process_inlines(
    grammar: &SyntaxGrammar,
    lexical_grammar: &LexicalGrammar,
) -> ProcessInlinesResult<InlinedProductionMap> {
    for symbol in &grammar.variables_to_inline {
        match symbol.kind {
            SymbolType::External => {
                Err(ProcessInlinesError::ExternalToken(
                    grammar.external_tokens[symbol.index].name.clone(),
                ))?;
            }
            SymbolType::Terminal => {
                Err(ProcessInlinesError::Token(
                    lexical_grammar.variables[symbol.index].name.clone(),
                ))?;
            }
            SymbolType::NonTerminal if symbol.index == 0 => {
                Err(ProcessInlinesError::FirstRule(
                    grammar.variables[symbol.index].name.clone(),
                ))?;
            }
            _ => {}
        }
    }

    Ok(InlinedProductionMapBuilder {
        productions: Vec::new(),
        production_indices_by_step_id: FxHashMap::default(),
    }
    .build(grammar))
}
