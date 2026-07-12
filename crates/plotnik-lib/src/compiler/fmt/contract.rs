#[cfg(debug_assertions)]
use std::collections::HashSet;

use super::ir::FormatFile;

pub(super) fn validate_model(file: &FormatFile) {
    let mut ids = vec![false; file.comment_count];
    let mut visited = 0;
    #[cfg(debug_assertions)]
    let mut slots = HashSet::with_capacity(file.comment_count);
    file.root.for_each_descendant_comment(&mut |comment| {
        let id = comment.id.0 as usize;
        let _attachment = comment.slot;
        assert!(id < ids.len(), "CommentId is in the normalized file");
        assert!(!ids[id], "CommentId is unique");
        ids[id] = true;
        #[cfg(debug_assertions)]
        assert!(slots.insert(comment.slot), "comment attachment is unique");
        visited += 1;
    });
    assert_eq!(visited, file.comment_count);
}

#[cfg(debug_assertions)]
pub(super) fn validate_rendered_comments(file: &FormatFile, output: &str) {
    use crate::compiler::parse::cst::SyntaxKind;
    use crate::compiler::parse::lex;

    let mut expected = Vec::with_capacity(file.comment_count);
    file.root
        .for_each_descendant_comment(&mut |comment| expected.push(comment.text().to_owned()));
    let actual = lex(output)
        .into_iter()
        .filter(|token| {
            matches!(
                token.kind,
                SyntaxKind::LineComment | SyntaxKind::BlockComment
            )
        })
        .map(|token| {
            let range = std::ops::Range::<usize>::from(token.span);
            output[range].to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected, "comments retain normalized source order");
}

#[cfg(not(debug_assertions))]
pub(super) fn validate_rendered_comments(_file: &FormatFile, _output: &str) {}
