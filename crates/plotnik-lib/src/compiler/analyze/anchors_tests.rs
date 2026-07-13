use crate::bytecode::Nav;
use crate::core::NodeClass;

use super::anchors::GapClass;

#[test]
fn admits_mirrors_vm_skip_policy() {
    // (anonymous, extra) axes are independent; the four kinds of node are
    // named-structural, anonymous-token, named-extra, anonymous-extra.
    let named = NodeClass {
        anonymous: false,
        extra: false,
    };
    let anon = NodeClass {
        anonymous: true,
        extra: false,
    };
    let named_extra = NodeClass {
        anonymous: false,
        extra: true,
    };
    let anon_extra = NodeClass {
        anonymous: true,
        extra: true,
    };

    // Any skips everything, even a named structural sibling.
    assert!(GapClass::Any.admits(named));
    assert!(GapClass::Any.admits(anon));

    // Broad skip = the VM's `is_trivia` = anonymous || extra.
    assert!(!GapClass::AnonymousAndExtras.admits(named));
    assert!(GapClass::AnonymousAndExtras.admits(anon));
    assert!(GapClass::AnonymousAndExtras.admits(named_extra));
    assert!(GapClass::AnonymousAndExtras.admits(anon_extra));

    // Narrow skip = extras only.
    assert!(!GapClass::ExtrasOnly.admits(named));
    assert!(!GapClass::ExtrasOnly.admits(anon));
    assert!(GapClass::ExtrasOnly.admits(named_extra));

    // Exact admits no skipped node.
    assert!(!GapClass::Exact.admits(anon));
    assert!(!GapClass::Exact.admits(named_extra));
}

#[test]
fn from_nav_projects_skip_suffix() {
    assert_eq!(GapClass::from_nav(Nav::Next), Some(GapClass::Any));
    assert_eq!(GapClass::from_nav(Nav::Down), Some(GapClass::Any));
    assert_eq!(GapClass::from_nav(Nav::Up(1)), Some(GapClass::Any));

    assert_eq!(
        GapClass::from_nav(Nav::NextSkip),
        Some(GapClass::AnonymousAndExtras)
    );
    assert_eq!(
        GapClass::from_nav(Nav::DownSkip),
        Some(GapClass::AnonymousAndExtras)
    );
    // `UpSkipTrivia` is the broad skip despite its name (mirrors `is_trivia`).
    assert_eq!(
        GapClass::from_nav(Nav::UpSkipTrivia(1)),
        Some(GapClass::AnonymousAndExtras)
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

    assert_eq!(GapClass::from_nav(Nav::NextExact), Some(GapClass::Exact));
    assert_eq!(GapClass::from_nav(Nav::DownExact), Some(GapClass::Exact));
    assert_eq!(GapClass::from_nav(Nav::UpExact(1)), Some(GapClass::Exact));

    // Control-flow navs open no sibling gap.
    assert_eq!(GapClass::from_nav(Nav::Epsilon), None);
    assert_eq!(GapClass::from_nav(Nav::Stay), None);
    assert_eq!(GapClass::from_nav(Nav::StayExact), None);
}
