//! Lowering for built-in text and boolean capture types.

use crate::bytecode::{EffectKind, Nav, SpanKind};
use crate::compiler::analyze::types::{
    CaptureTypePlan, CaptureTypePlanKind, OptionMode, TerminalData,
};
use crate::compiler::ids::DefId;
use crate::compiler::lower::ir::{
    CalleeEntry, DefBodyMode, DefVariant, EffectIR, Label, ReturnAddr, SplitReturnAddrs,
};
use crate::compiler::lower::spans::{SpanBindingIR, SpanId};
use crate::compiler::parse::ast::{self, Pattern, QuantifierKind};

use super::NfaBuilder;
use super::capture::{CaptureEffects, PatternCtx};
use super::nfa_emit::{ForkTargets, Greediness};
use super::quantifier::{QuantifierForm, classify_quantifier};
use super::scope::{CaptureExits, SkipExit};

#[derive(Clone)]
enum ValueDestination {
    Pending,
    ListItem,
    Effects(Vec<EffectIR>),
}

impl ValueDestination {
    fn followed_by(self, trailing: &[EffectIR]) -> Self {
        let mut effects = self.into_effects();
        effects.extend_from_slice(trailing);
        Self::Effects(effects)
    }

    fn into_effects(self) -> Vec<EffectIR> {
        match self {
            Self::Pending => vec![],
            Self::ListItem => vec![EffectIR::array_push()],
            Self::Effects(effects) => effects,
        }
    }
}

struct CaptureBinding {
    entry: Vec<EffectIR>,
    destination: ValueDestination,
}

#[derive(Clone, Copy)]
enum CaptureTerminal {
    Text(TerminalData),
    Presence(TerminalData),
}

impl CaptureTerminal {
    fn is_presence(self) -> bool {
        matches!(self, Self::Presence(_))
    }

    fn data(self) -> TerminalData {
        match self {
            Self::Text(data) | Self::Presence(data) => data,
        }
    }

    fn close(self) -> EffectIR {
        match self {
            Self::Text(_) => EffectIR::str_close(),
            Self::Presence(_) => EffectIR::bool_close(true),
        }
    }

    fn node_value(self) -> EffectIR {
        match self {
            Self::Text(_) => EffectIR::node_str(),
            Self::Presence(_) => EffectIR::node_bool(),
        }
    }
}

/// Owns one capture-type transformation and its continuation protocol.
///
/// The plan, navigation, and match/skip exits travel together because every
/// recursive call changes them as one semantic route. Keeping that behavior on
/// a lowering role prevents helper calls from accepting incoherent combinations
/// such as a string close with presence-boolean source semantics.
pub(super) struct CaptureTypeLowerer<'b, 'a> {
    compiler: &'b mut NfaBuilder<'a>,
    plan: CaptureTypePlan,
    nav: Option<Nav>,
    exits: CaptureExits,
}

impl<'a> NfaBuilder<'a> {
    pub(super) fn capture_type<'b>(
        &'b mut self,
        plan: &CaptureTypePlan,
        nav: Option<Nav>,
        exits: CaptureExits,
    ) -> CaptureTypeLowerer<'b, 'a> {
        CaptureTypeLowerer {
            compiler: self,
            plan: plan.clone(),
            nav,
            exits,
        }
    }

    /// A transformed quantifier element still obeys the ordinary iteration
    /// contract: matching zero nodes is a failed attempt, never a present
    /// optional value or a list item. Apply the conversion only to the
    /// consuming continuation.
    fn capture_type_iteration_exits(&self, pattern: &Pattern, exit: Label) -> CaptureExits {
        if !self.pattern_is_nullable(pattern) {
            return CaptureExits::Single(exit);
        }

        CaptureExits::Split {
            match_exit: exit,
            skip_exit: SkipExit::Fail,
        }
    }

    fn with_terminal_data<T>(
        &mut self,
        data: TerminalData,
        compile: impl FnOnce(&mut Self) -> T,
    ) -> T {
        if data == TerminalData::Semantic {
            return self.with_suppression(compile);
        }
        compile(self)
    }

    fn capture_type_binding(&mut self, capture: &ast::CapturedPattern) -> CaptureBinding {
        let name = capture
            .name()
            .expect("regular capture has a validated name");
        let member = self
            .lookup_member_in_scope(&name.text()[1..])
            .expect("capture field resolves in its output scope");
        let mut entry = Vec::new();
        let mut effects = vec![EffectIR::with_member(EffectKind::RecordSet, member)];
        if let Some(id) = self.span_id(capture.syntax(), SpanKind::Capture) {
            self.bind_span(id, SpanBindingIR::Member(member));
            entry.push(EffectIR::span_start(id.0));
            effects.push(EffectIR::span_end(id.0));
        }
        CaptureBinding {
            entry,
            destination: ValueDestination::Effects(effects),
        }
    }
}

impl CaptureTypeLowerer<'_, '_> {
    pub(super) fn definition(mut self, body: &Pattern) -> Label {
        self.lower(body, ValueDestination::Pending)
    }

    pub(super) fn captured(
        mut self,
        capture: &ast::CapturedPattern,
        outer: CaptureEffects,
    ) -> Label {
        let inner = capture
            .inner()
            .expect("a capture-type transformation has an ordinary captured value");
        let CaptureEffects { mut pre, post } = outer;
        let binding = self.compiler.capture_type_binding(capture);
        pre.extend(binding.entry);
        let destination = binding.destination.followed_by(&post);
        let entry = self.lower(&inner, destination);
        self.compiler.wrap_entry_pre(entry, pre)
    }

    pub(super) fn list_item(mut self, inner: &Pattern) -> Label {
        self.exits = self
            .compiler
            .capture_type_iteration_exits(inner, self.exits.match_exit());
        self.lower(inner, ValueDestination::ListItem)
    }

    fn lower(&mut self, pattern: &Pattern, destination: ValueDestination) -> Label {
        if matches!(pattern, Pattern::DefRef(_)) {
            return self.specialized_reference(pattern, destination);
        }

        match self.plan.kind().clone() {
            CaptureTypePlanKind::TextTerminal { data, .. } => {
                self.capture_terminal(pattern, destination, CaptureTerminal::Text(data))
            }
            CaptureTypePlanKind::BoolTerminal { data } => {
                self.capture_terminal(pattern, destination, CaptureTerminal::Presence(data))
            }
            CaptureTypePlanKind::Option { mode, inner } => {
                let Pattern::QuantifiedPattern(quant) = pattern else {
                    return self.specialized_reference(pattern, destination);
                };
                self.optional(quant, mode, *inner, destination)
            }
            CaptureTypePlanKind::List { element, .. } => {
                let Pattern::QuantifiedPattern(quant) = pattern else {
                    return self.specialized_reference(pattern, destination);
                };
                self.list(quant, *element, destination)
            }
        }
    }

    fn capture_terminal(
        &mut self,
        pattern: &Pattern,
        destination: ValueDestination,
        terminal: CaptureTerminal,
    ) -> Label {
        if terminal.is_presence() && !self.compiler.records_inspection() {
            return self.bool_without_provenance(pattern, destination, terminal.data());
        }

        if terminal.data() == TerminalData::NodeRepresentation
            && matches!(pattern, Pattern::NodePattern(_) | Pattern::TokenPattern(_))
        {
            return self.node_terminal(pattern, destination, terminal);
        }

        let match_close = self.close_scalar(self.exits.match_exit(), destination.clone(), terminal);
        let entry = match self.exits {
            CaptureExits::Single(_) => {
                let nav = self.nav;
                self.compiler.with_source_marking(|this| {
                    this.with_terminal_data(terminal.data(), |this| {
                        this.dispatch_pattern(pattern, PatternCtx::with_nav(match_close, nav))
                    })
                })
            }
            CaptureExits::Split { skip_exit, .. } => {
                let skip_exit = match skip_exit {
                    SkipExit::To(skip) => {
                        SkipExit::To(self.close_scalar(skip, destination, terminal))
                    }
                    SkipExit::Fail => SkipExit::Fail,
                };
                let nav = self.nav;
                self.compiler.with_source_marking(|this| {
                    this.with_terminal_data(terminal.data(), |this| {
                        let pattern_ctx = PatternCtx {
                            exit: match_close,
                            nav,
                            capture: CaptureEffects::default(),
                            value: false,
                        };
                        this.compile_nullable_pattern(pattern, pattern_ctx, skip_exit)
                    })
                })
            }
        };
        self.compiler.emit_effects_epsilon(
            entry,
            vec![EffectIR::scalar_open()],
            CaptureEffects::default(),
        )
    }

    fn bool_without_provenance(
        &mut self,
        pattern: &Pattern,
        destination: ValueDestination,
        data: TerminalData,
    ) -> Label {
        let match_exit = self.bool_value(self.exits.match_exit(), destination.clone(), true);
        match self.exits {
            CaptureExits::Single(_) => {
                let nav = self.nav;
                self.compiler.with_terminal_data(data, |this| {
                    this.dispatch_pattern(pattern, PatternCtx::with_nav(match_exit, nav))
                })
            }
            CaptureExits::Split { skip_exit, .. } => {
                let skip_exit = match skip_exit {
                    SkipExit::To(skip) => SkipExit::To(self.bool_value(skip, destination, true)),
                    SkipExit::Fail => SkipExit::Fail,
                };
                let nav = self.nav;
                self.compiler.with_terminal_data(data, |this| {
                    let pattern_ctx = PatternCtx {
                        exit: match_exit,
                        nav,
                        capture: CaptureEffects::default(),
                        value: false,
                    };
                    this.compile_nullable_pattern(pattern, pattern_ctx, skip_exit)
                })
            }
        }
    }

    fn bool_value(&mut self, exit: Label, destination: ValueDestination, value: bool) -> Label {
        let mut effects = vec![EffectIR::bool_value(value)];
        effects.extend(destination.into_effects());
        self.compiler
            .emit_effects_epsilon(exit, effects, CaptureEffects::default())
    }

    fn node_terminal(
        &mut self,
        pattern: &Pattern,
        destination: ValueDestination,
        terminal: CaptureTerminal,
    ) -> Label {
        let mut effects = vec![terminal.node_value()];
        effects.extend(destination.into_effects());
        let pattern_ctx = PatternCtx {
            exit: self.exits.match_exit(),
            nav: self.nav,
            capture: CaptureEffects::new_post(effects),
            value: false,
        };
        self.compiler.dispatch_pattern(pattern, pattern_ctx)
    }

    fn close_scalar(
        &mut self,
        exit: Label,
        destination: ValueDestination,
        terminal: CaptureTerminal,
    ) -> Label {
        let mut effects = vec![terminal.close()];
        effects.extend(destination.into_effects());
        self.compiler
            .emit_effects_epsilon(exit, effects, CaptureEffects::default())
    }

    fn optional(
        &mut self,
        quant: &ast::QuantifiedPattern,
        mode: OptionMode,
        inner_plan: CaptureTypePlan,
        destination: ValueDestination,
    ) -> Label {
        let QuantifierForm::Quantified { inner, kind } = classify_quantifier(quant) else {
            unreachable!("an optional capture-type plan has an optional quantifier")
        };
        assert_eq!(kind.kind(), QuantifierKind::Optional);

        let (match_exit, skip_exit) = match self.exits {
            CaptureExits::Single(exit) => (exit, SkipExit::To(exit)),
            CaptureExits::Split {
                match_exit,
                skip_exit,
            } => (match_exit, skip_exit),
        };
        let nav = self.nav.unwrap_or(Nav::Down);
        let matched = self
            .compiler
            .emit_iteration(nav, match_exit, |this, target| {
                let inner_exits = this.capture_type_iteration_exits(&inner, target.exit);
                this.capture_type(&inner_plan, Some(target.nav), inner_exits)
                    .lower(&inner, destination.clone())
            });

        let skipped = match skip_exit {
            SkipExit::To(exit) => Some(match mode {
                OptionMode::Preserve => {
                    let mut effects = vec![EffectIR::absent()];
                    effects.extend(destination.into_effects());
                    self.compiler
                        .emit_effects_epsilon(exit, effects, CaptureEffects::default())
                }
                OptionMode::Bool => {
                    let mut effects = vec![EffectIR::bool_value(false)];
                    effects.extend(destination.into_effects());
                    self.compiler
                        .emit_effects_epsilon(exit, effects, CaptureEffects::default())
                }
            }),
            SkipExit::Fail => None,
        };

        match skipped {
            Some(skipped) => self.compiler.emit_fork_epsilon(
                ForkTargets {
                    prefer: matched,
                    other: skipped,
                },
                Greediness::from(kind),
            ),
            None => matched,
        }
    }

    fn list(
        &mut self,
        quant: &ast::QuantifiedPattern,
        element: CaptureTypePlan,
        destination: ValueDestination,
    ) -> Label {
        let close = |compiler: &mut NfaBuilder<'_>, exit, destination: ValueDestination| {
            let mut effects = vec![EffectIR::list_close()];
            effects.extend(destination.into_effects());
            compiler.emit_effects_epsilon(exit, effects, CaptureEffects::default())
        };
        let closed_exits = match self.exits {
            CaptureExits::Single(exit) => {
                CaptureExits::Single(close(self.compiler, exit, destination))
            }
            CaptureExits::Split {
                match_exit,
                skip_exit,
            } => CaptureExits::Split {
                match_exit: close(self.compiler, match_exit, destination.clone()),
                skip_exit: match skip_exit {
                    SkipExit::To(skip) => SkipExit::To(close(self.compiler, skip, destination)),
                    SkipExit::Fail => SkipExit::Fail,
                },
            },
        };
        let iterations = self.compiler.compile_capture_type_list_iterations(
            quant,
            element,
            closed_exits,
            self.nav,
        );
        self.compiler.emit_effects_epsilon(
            iterations,
            vec![EffectIR::list_open()],
            CaptureEffects::default(),
        )
    }

    fn specialized_reference(&mut self, pattern: &Pattern, destination: ValueDestination) -> Label {
        let Pattern::DefRef(reference) = pattern else {
            unreachable!("only a definition reference needs output specialization")
        };
        let def_id = self.compiler.resolve_ref_def_id(reference);
        if self.compiler.nullable_defs.contains(&def_id) {
            return self.nullable_reference(def_id, destination);
        }

        let mode = DefBodyMode::ordinary().with_capture_type(self.plan.clone());
        let mode = self.compiler.propagate_source_mode(mode);
        let target = self
            .compiler
            .ensure_def_variant(DefVariant::new(def_id, mode));
        let exit = self
            .compiler
            .emit_effects_if_nonempty(self.exits.match_exit(), destination.into_effects());
        self.compiler.emit_call(
            self.nav.unwrap_or(Nav::Stay),
            None,
            ReturnAddr(exit),
            CalleeEntry(target),
        )
    }

    fn nullable_reference(&mut self, def_id: DefId, destination: ValueDestination) -> Label {
        if self.compiler.inline_stack.contains(&def_id) {
            return self.guarded_reference(def_id, destination);
        }

        let name = self.compiler.ctx.analysis.interner.resolve(
            self.compiler
                .ctx
                .analysis
                .dependency_analysis
                .def_name_sym(def_id),
        );
        let body = self
            .compiler
            .ctx
            .symbol_table
            .body(name)
            .expect("analyzed definition has a body");
        let output = self
            .compiler
            .ctx
            .analysis
            .type_analysis
            .expect_def_output(def_id);
        let destination_effects = destination.into_effects();
        let destination_exits = self.exits.map_targets(|exit| {
            self.compiler
                .emit_effects_if_nonempty(exit, destination_effects.clone())
        });
        let (body_exits, def_span) = self.bracket_definition_exits(body, destination_exits);
        let plan = self.plan.clone();
        let nav = self.nav;

        self.compiler.inline_stack.push(def_id);
        let entry = self.compiler.with_scope(output, |this| {
            this.capture_type(&plan, nav, body_exits)
                .lower(body, ValueDestination::Pending)
        });
        self.compiler.inline_stack.pop();
        self.compiler.wrap_def_body_entry(entry, def_span)
    }

    fn bracket_definition_exits(
        &mut self,
        body: &Pattern,
        exits: CaptureExits,
    ) -> (CaptureExits, Option<SpanId>) {
        match exits {
            CaptureExits::Single(exit) => {
                let (exit, span) = self.compiler.bracket_def_body_exit(body, exit);
                (CaptureExits::Single(exit), span)
            }
            CaptureExits::Split {
                match_exit,
                skip_exit,
            } => {
                let (match_exit, span) = self.compiler.bracket_def_body_exit(body, match_exit);
                let skip_exit = match skip_exit {
                    SkipExit::To(exit) => {
                        SkipExit::To(self.compiler.bracket_def_body_exit(body, exit).0)
                    }
                    SkipExit::Fail => SkipExit::Fail,
                };
                (
                    CaptureExits::Split {
                        match_exit,
                        skip_exit,
                    },
                    span,
                )
            }
        }
    }

    fn guarded_reference(&mut self, def_id: DefId, destination: ValueDestination) -> Label {
        let entry_nav = self.nav.unwrap_or(Nav::Stay);
        let CaptureExits::Split {
            match_exit,
            skip_exit,
        } = self.exits
        else {
            let CaptureExits::Single(exit) = self.exits else {
                unreachable!("capture exits are single or split")
            };
            let continuation = self
                .compiler
                .emit_effects_if_nonempty(exit, destination.into_effects());
            return self.split_guarded_reference(def_id, entry_nav, continuation, continuation);
        };

        let SkipExit::To(zero_exit) = skip_exit else {
            let mode = DefBodyMode::ordinary().with_capture_type(self.plan.clone());
            let mode = self.compiler.propagate_source_mode(mode);
            let target = self
                .compiler
                .ensure_def_variant(DefVariant::routed_match(def_id, mode, entry_nav));
            let continuation = self
                .compiler
                .emit_effects_if_nonempty(match_exit, destination.into_effects());
            return self.compiler.emit_routed_call(
                entry_nav,
                ReturnAddr(continuation),
                CalleeEntry(target),
            );
        };

        let matched_return = self
            .compiler
            .emit_effects_if_nonempty(match_exit, destination.clone().into_effects());
        let empty_return = self
            .compiler
            .emit_effects_if_nonempty(zero_exit, destination.into_effects());
        self.split_guarded_reference(def_id, entry_nav, matched_return, empty_return)
    }

    fn split_guarded_reference(
        &mut self,
        def_id: DefId,
        entry_nav: Nav,
        matched_return: Label,
        empty_return: Label,
    ) -> Label {
        let mode = DefBodyMode::ordinary().with_capture_type(self.plan.clone());
        let mode = self.compiler.propagate_source_mode(mode);
        let target = self
            .compiler
            .ensure_def_variant(DefVariant::routed_split(def_id, mode, entry_nav));
        self.compiler.emit_split_call(
            entry_nav,
            SplitReturnAddrs {
                matched: ReturnAddr(matched_return),
                empty: ReturnAddr(empty_return),
            },
            CalleeEntry(target),
        )
    }
}
