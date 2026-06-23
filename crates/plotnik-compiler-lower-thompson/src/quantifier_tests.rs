use super::quantifier::quantifier_search_nav;
use plotnik_bytecode::Nav;

/// Every `Nav` variant, with a representative level for the `Up*` family.
const ALL_NAVS: &[Nav] = &[
    Nav::Epsilon,
    Nav::Stay,
    Nav::StayExact,
    Nav::Next,
    Nav::NextSkip,
    Nav::NextSkipExtras,
    Nav::NextExact,
    Nav::Down,
    Nav::DownSkip,
    Nav::DownSkipExtras,
    Nav::DownExact,
    Nav::Up(1),
    Nav::UpSkipTrivia(1),
    Nav::UpSkipExtras(1),
    Nav::UpExact(1),
];

/// `emit_loop_iterations` relies on this: a nav that begins a quantifier search
/// must repeat as a search, so its `.expect("a search nav repeats as a search")`
/// can never fire. The invariant straddles two crates — `Nav::sibling_continuation`
/// (plotnik-bytecode) and `quantifier_search_nav` (here) — so neither side's
/// compiler can catch a future divergence; this test does.
#[test]
fn search_nav_repeats_as_search() {
    for &nav in ALL_NAVS {
        if quantifier_search_nav(nav).is_some() {
            let repeat = nav.sibling_continuation();
            assert!(
                quantifier_search_nav(repeat).is_some(),
                "search nav {nav:?} repeats as non-search {repeat:?}",
            );
        }
    }
}
