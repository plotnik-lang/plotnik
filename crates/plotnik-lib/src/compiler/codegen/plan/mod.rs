//! Target-neutral plans consumed by generated-code backends.
//!
//! Lowering decides what the query means. This layer turns that meaning into
//! plain, ordered data so source renderers only choose a target language's
//! representation and syntax.

mod matcher;

pub(crate) use matcher::{
    CallPlan, CheckPlan, EffectPlan, FlowPlan, KindClass, MatchPlan, MatcherPlan, ModulePlan,
    PredicatePlan, PredicateValuePlan, RegexId, StateId, StateOrigin, StatePlan, StatePlanKind,
};
