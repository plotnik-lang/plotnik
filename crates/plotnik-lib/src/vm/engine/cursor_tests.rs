//! Direct tests for `CursorWrapper::go_up` (driven through the public `navigate`).
//!
//! `go_up` is the heart of the #471 fix: a same-mode `Up*` ascent must validate
//! its exit constraint at *every* level it leaves, which is what lets
//! `collapse_up` merge nested trailing anchors (`Up*(a)` then `Up*(b)` == `Up*(a+b)`).
//! It must also leave the cursor where it started when it fails. The conformance
//! suite can only observe match/no-match — the VM backtracks to a checkpoint on
//! failure, masking whether `go_up` itself restored — so the restore contract is
//! verified here, against real trees, by inspecting the cursor after the call.

use crate::bytecode::Nav;
use arborium_tree_sitter::{Language, Parser, Tree, TreeCursor};

use super::cursor::CursorWrapper;

fn parse_js(source: &str) -> Tree {
    let language: Language = arborium_javascript::language().into();
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .expect("set javascript language");
    parser.parse(source, None).expect("parse javascript source")
}

/// Root-relative descendant index of the leaf token whose source text is `text`,
/// so a test can position the cursor on a known node without depending on the
/// exact shape of the intervening tree.
fn leaf_index(tree: &Tree, source: &str, text: &str) -> u32 {
    fn walk(c: &mut TreeCursor, source: &str, text: &str, found: &mut Option<u32>) {
        let n = c.node();
        if n.child_count() == 0 && &source[n.start_byte()..n.end_byte()] == text {
            *found = Some(c.descendant_index() as u32);
            return;
        }
        if c.goto_first_child() {
            loop {
                walk(c, source, text, found);
                if found.is_some() || !c.goto_next_sibling() {
                    break;
                }
            }
            c.goto_parent();
        }
    }

    let mut c = tree.walk();
    let mut found = None;
    walk(&mut c, source, text, &mut found);
    found.unwrap_or_else(|| panic!("no leaf with text {text:?}"))
}

/// Source text spanned by the cursor's current node.
fn text<'a>(w: &CursorWrapper<'_>, source: &'a str) -> &'a str {
    let n = w.node();
    &source[n.start_byte()..n.end_byte()]
}

#[test]
fn snapshot_restore_returns_to_saved_position() {
    let source = "[1, 2]";
    let tree = parse_js(source);
    let mut w = CursorWrapper::new(tree.walk());
    let saved = leaf_index(&tree, source, "1");
    let away = leaf_index(&tree, source, "2");
    w.activate_snapshots_for_test();
    w.goto_descendant(saved);
    let snapshot = w.snapshot(1);

    w.goto_descendant(away);
    w.restore(snapshot, saved);

    assert_eq!(w.descendant_index(), saved);
    assert_eq!(text(&w, source), "1");
    assert_eq!(w.snapshot_live_len(), 0, "single-use snapshot is freed");
}

#[test]
fn snapshot_restore_falls_back_after_eviction() {
    let source = "[1, 2]";
    let tree = parse_js(source);
    let mut w = CursorWrapper::new(tree.walk());
    let saved = leaf_index(&tree, source, "1");
    let away = leaf_index(&tree, source, "2");
    w.activate_snapshots_for_test();
    w.goto_descendant(saved);
    let evicted = w.snapshot(1);
    let mut newer = Vec::new();

    for _ in 0..CursorWrapper::snapshot_cap() {
        newer.push(w.snapshot(1));
    }
    assert_eq!(
        w.snapshot_live_len(),
        CursorWrapper::snapshot_cap(),
        "oldest snapshot was evicted once the cap was exceeded"
    );

    for snapshot in newer.into_iter().rev() {
        w.restore(snapshot, saved);
    }
    assert_eq!(w.snapshot_live_len(), 0);

    w.goto_descendant(away);
    w.restore(evicted, saved);

    assert_eq!(w.descendant_index(), saved);
    assert_eq!(text(&w, source), "1");
}

#[test]
fn snapshot_refs_are_shared_across_fanout() {
    let source = "[1, 2, 3, 4]";
    let tree = parse_js(source);
    let mut w = CursorWrapper::new(tree.walk());
    let saved = leaf_index(&tree, source, "1");
    w.activate_snapshots_for_test();
    w.goto_descendant(saved);
    let snapshot = w.snapshot(3);

    for (i, away) in ["2", "3", "4"].into_iter().enumerate() {
        let away = leaf_index(&tree, source, away);
        w.goto_descendant(away);
        w.restore(snapshot, saved);

        assert_eq!(w.descendant_index(), saved);
        assert_eq!(text(&w, source), "1");
        let expected_live = if i == 2 { 0 } else { 1 };
        assert_eq!(
            w.snapshot_live_len(),
            expected_live,
            "shared snapshot should survive until its last reference"
        );
    }
}

#[test]
fn same_position_snapshot_restore_releases_reference() {
    let source = "[1]";
    let tree = parse_js(source);
    let mut w = CursorWrapper::new(tree.walk());
    let saved = leaf_index(&tree, source, "1");
    w.activate_snapshots_for_test();
    w.goto_descendant(saved);
    let snapshot = w.snapshot(1);

    w.restore(snapshot, saved);

    assert_eq!(w.descendant_index(), saved);
    assert_eq!(text(&w, source), "1");
    assert_eq!(
        w.snapshot_live_len(),
        0,
        "same-position restore still consumes the checkpoint reference"
    );
}

#[test]
fn snapshots_activate_after_lateral_restore_pressure() {
    let source = format!(
        "[{}]",
        (0..80)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let tree = parse_js(&source);
    let mut w = CursorWrapper::new(tree.walk());
    let saved = leaf_index(&tree, &source, "0");
    let away = leaf_index(&tree, &source, "79");
    w.goto_descendant(saved);

    assert!(
        w.snapshot(1).is_none(),
        "cold scans should not pay snapshot creation cost"
    );

    for _ in 0..CursorWrapper::snapshot_activation_misses() {
        w.goto_descendant(away);
        w.restore(None, saved);
    }

    assert!(
        w.snapshot(1).is_some(),
        "lateral restore pressure should activate snapshot creation"
    );
}

#[test]
fn go_up_skip_trivia_rejects_when_a_parent_is_not_last_child() {
    // `1` is the last non-trivia child of the inner array, but the inner array is
    // NOT the last non-trivia child of the outer array (`x` follows). A per-level
    // check must reject; a single check at the innermost level would wrongly pass.
    let source = "[[1], x]";
    let tree = parse_js(source);
    let mut w = CursorWrapper::new(tree.walk());
    w.goto_descendant(leaf_index(&tree, source, "1"));
    let origin = w.descendant_index();

    let result = w.navigate(Nav::UpSkipTrivia(2));

    assert!(
        result.is_none(),
        "outer level is not last → ascent must fail"
    );
    assert_eq!(
        w.descendant_index(),
        origin,
        "cursor must be restored on failure"
    );
    assert_eq!(text(&w, source), "1");
}

#[test]
fn go_up_skip_trivia_matches_when_every_level_is_last() {
    // Every level is last (only `]` follows at each), so the two-level ascent holds.
    let source = "[[1]]";
    let tree = parse_js(source);
    let mut w = CursorWrapper::new(tree.walk());
    w.goto_descendant(leaf_index(&tree, source, "1"));

    let result = w.navigate(Nav::UpSkipTrivia(2));

    assert!(
        result.is_some(),
        "every level is last → ascent must succeed"
    );
    assert_eq!(w.node().kind(), "array");
    assert_eq!(text(&w, source), "[[1]]", "cursor ends two levels up");
}

#[test]
fn go_up_any_ignores_the_constraint() {
    // `Up` (no constraint) ascends regardless of last-child-ness, even where
    // `UpSkipTrivia(2)` would reject (the same `[[1], x]` tree as above).
    let source = "[[1], x]";
    let tree = parse_js(source);
    let mut w = CursorWrapper::new(tree.walk());
    w.goto_descendant(leaf_index(&tree, source, "1"));

    let result = w.navigate(Nav::Up(2));

    assert!(result.is_some(), "Up checks nothing → always ascends");
    assert_eq!(text(&w, source), "[[1], x]", "cursor ends two levels up");
}

#[test]
fn go_up_single_level_ascends_one() {
    // The level-1 ascent only checks the innermost level, which holds.
    let source = "[[1], x]";
    let tree = parse_js(source);
    let mut w = CursorWrapper::new(tree.walk());
    w.goto_descendant(leaf_index(&tree, source, "1"));

    let result = w.navigate(Nav::UpSkipTrivia(1));

    assert!(result.is_some());
    assert_eq!(
        text(&w, source),
        "[1]",
        "cursor ends one level up (inner array)"
    );
}

#[test]
fn go_up_exact_rejects_when_a_parent_is_not_last_child() {
    // `x` is the exact-last child of `return_statement`, but `return_statement` is
    // NOT the exact-last child of the block (`}` follows). The strict (`Exact`)
    // per-level check must reject at the outer level.
    let source = "function f(){ return x }";
    let tree = parse_js(source);
    let mut w = CursorWrapper::new(tree.walk());
    w.goto_descendant(leaf_index(&tree, source, "x"));
    let origin = w.descendant_index();

    let result = w.navigate(Nav::UpExact(2));

    assert!(
        result.is_none(),
        "the closing brace follows the return → ascent must fail"
    );
    assert_eq!(
        w.descendant_index(),
        origin,
        "cursor must be restored on failure"
    );
    assert_eq!(text(&w, source), "x");
}

#[test]
fn go_up_checks_all_three_levels() {
    // Three stacked anchors collapse to `UpSkipTrivia(3)`; all three levels are
    // last, so the deep ascent succeeds and lands on the outermost array.
    let source = "[[[1]]]";
    let tree = parse_js(source);
    let mut w = CursorWrapper::new(tree.walk());
    w.goto_descendant(leaf_index(&tree, source, "1"));

    let result = w.navigate(Nav::UpSkipTrivia(3));

    assert!(result.is_some());
    assert_eq!(text(&w, source), "[[[1]]]", "cursor ends three levels up");
}

#[test]
fn go_up_past_the_root_fails_and_restores() {
    // Ascending more levels than the tree is deep must fail when `goto_parent`
    // runs out, and still restore the cursor to where it started.
    let source = "[[1]]";
    let tree = parse_js(source);
    let mut w = CursorWrapper::new(tree.walk());
    w.goto_descendant(leaf_index(&tree, source, "1"));
    let origin = w.descendant_index();

    let result = w.navigate(Nav::Up(50));

    assert!(result.is_none(), "cannot ascend past the root");
    assert_eq!(
        w.descendant_index(),
        origin,
        "cursor must be restored on failure"
    );
    assert_eq!(text(&w, source), "1");
}
