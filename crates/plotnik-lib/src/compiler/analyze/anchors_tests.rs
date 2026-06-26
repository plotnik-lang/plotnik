use crate::bytecode::Nav;

use super::anchors::GapClass;

#[test]
fn admits_mirrors_vm_skip_policy() {
    // (anonymous, extra) axes are independent; the four kinds of node are
    // named-structural, anonymous-token, named-extra, anonymous-extra.
    let named = (false, false);
    let anon = (true, false);
    let named_extra = (false, true);
    let anon_extra = (true, true);

    // Any skips everything, even a named structural sibling.
    assert!(GapClass::Any.admits(named.0, named.1));
    assert!(GapClass::Any.admits(anon.0, anon.1));

    // Broad skip = the VM's `is_trivia` = anonymous || extra.
    assert!(!GapClass::AnonAndExtras.admits(named.0, named.1));
    assert!(GapClass::AnonAndExtras.admits(anon.0, anon.1));
    assert!(GapClass::AnonAndExtras.admits(named_extra.0, named_extra.1));
    assert!(GapClass::AnonAndExtras.admits(anon_extra.0, anon_extra.1));

    // Narrow skip = extras only.
    assert!(!GapClass::ExtrasOnly.admits(named.0, named.1));
    assert!(!GapClass::ExtrasOnly.admits(anon.0, anon.1));
    assert!(GapClass::ExtrasOnly.admits(named_extra.0, named_extra.1));

    // Strict admits nothing.
    assert!(!GapClass::Nothing.admits(anon.0, anon.1));
    assert!(!GapClass::Nothing.admits(named_extra.0, named_extra.1));
}

#[test]
fn from_nav_projects_skip_suffix() {
    assert_eq!(GapClass::from_nav(Nav::Next), Some(GapClass::Any));
    assert_eq!(GapClass::from_nav(Nav::Down), Some(GapClass::Any));
    assert_eq!(GapClass::from_nav(Nav::Up(1)), Some(GapClass::Any));

    assert_eq!(
        GapClass::from_nav(Nav::NextSkip),
        Some(GapClass::AnonAndExtras)
    );
    assert_eq!(
        GapClass::from_nav(Nav::DownSkip),
        Some(GapClass::AnonAndExtras)
    );
    // `UpSkipTrivia` is the broad skip despite its name (mirrors `is_trivia`).
    assert_eq!(
        GapClass::from_nav(Nav::UpSkipTrivia(1)),
        Some(GapClass::AnonAndExtras)
    );

    assert_eq!(
        GapClass::from_nav(Nav::NextSkipExtras),
        Some(GapClass::ExtrasOnly)
    );
    assert_eq!(
        GapClass::from_nav(Nav::DownSkipExtras),
        Some(GapClass::ExtrasOnly)
    );
    assert_eq!(
        GapClass::from_nav(Nav::UpSkipExtras(1)),
        Some(GapClass::ExtrasOnly)
    );

    assert_eq!(GapClass::from_nav(Nav::NextExact), Some(GapClass::Nothing));
    assert_eq!(GapClass::from_nav(Nav::DownExact), Some(GapClass::Nothing));
    assert_eq!(GapClass::from_nav(Nav::UpExact(1)), Some(GapClass::Nothing));

    // Control-flow navs open no sibling gap.
    assert_eq!(GapClass::from_nav(Nav::Epsilon), None);
    assert_eq!(GapClass::from_nav(Nav::Stay), None);
    assert_eq!(GapClass::from_nav(Nav::StayExact), None);
}
