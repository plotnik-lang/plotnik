use crate::bytecode::Nav;
use crate::compiler::parse::ast::Pattern;

pub use crate::compiler::analyze::anchors::AnchorSemantics;

pub fn pattern_owns_iteration(pattern: &Pattern) -> bool {
    quantifier_kind(pattern).is_some()
}

pub fn resumable_search_nav(nav: Option<Nav>) -> Option<Nav> {
    match nav {
        Some(nav @ (Nav::Down | Nav::Next | Nav::Stay)) => Some(nav),
        _ => None,
    }
}

pub fn is_down_nav(nav: Option<Nav>) -> bool {
    matches!(
        nav,
        Some(Nav::Down | Nav::DownSkip | Nav::DownSkipExtras | Nav::DownExact)
    )
}

fn quantifier_kind(pattern: &Pattern) -> Option<crate::compiler::parse::ast::QuantifierKind> {
    let pattern = match pattern {
        Pattern::CapturedPattern(cap) => cap.inner()?,
        e => e.clone(),
    };

    let Pattern::QuantifiedPattern(q) = &pattern else {
        return None;
    };
    q.quantifier_kind()
}
