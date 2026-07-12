use std::ops::Range;

use rowan::NodeOrToken;

use crate::compiler::parse::Root;
use crate::compiler::parse::cst::{SyntaxKind, SyntaxNode};

use super::comments::{Comment, CommentClassifier, CommentId, CommentSlot, UnitId};
use super::contract;
use super::ir::{Element, FormatFile, ModelNode, NodeKind, Token};
use super::measure;

pub(super) fn normalize(source: &str, root: &Root) -> FormatFile {
    let significant_ranges = root
        .syntax()
        .descendants_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .filter(|token| !token.kind().is_trivia())
        .map(|token| Range::<usize>::from(token.text_range()))
        .collect::<Vec<_>>();
    let mut builder = Builder {
        classifier: CommentClassifier::new(source, significant_ranges),
        next_comment_id: 0,
        next_unit_id: 0,
    };
    let root = builder.node(root.syntax());
    let file = FormatFile {
        root,
        comment_count: builder.next_comment_id as usize,
    };
    contract::validate_model(&file);
    file
}

struct Builder<'q> {
    classifier: CommentClassifier<'q>,
    next_comment_id: u32,
    next_unit_id: u32,
}

impl Builder<'_> {
    fn node(&mut self, syntax: &SyntaxNode) -> ModelNode {
        let owner = UnitId(self.next_unit_id);
        self.next_unit_id += 1;
        let elements: Vec<Element> = syntax
            .children_with_tokens()
            .enumerate()
            .filter_map(|(element_index, element)| match element {
                NodeOrToken::Node(node) => Some(Element::Node(Box::new(self.node(&node)))),
                NodeOrToken::Token(token)
                    if matches!(token.kind(), SyntaxKind::Whitespace | SyntaxKind::Newline) =>
                {
                    None
                }
                NodeOrToken::Token(token)
                    if matches!(
                        token.kind(),
                        SyntaxKind::LineComment | SyntaxKind::BlockComment
                    ) =>
                {
                    Some(Element::Comment(self.comment(
                        &token,
                        CommentSlot {
                            owner,
                            element_index: element_index as u32,
                        },
                    )))
                }
                NodeOrToken::Token(token) => {
                    let replacement = if token.kind() == SyntaxKind::Slash
                        && syntax.kind() != SyntaxKind::Regex
                    {
                        Some("#")
                    } else {
                        None
                    };
                    Some(Element::Token(Token::new(token, replacement)))
                }
            })
            .collect();
        let kind = NodeKind::from_syntax(syntax.kind(), &elements);
        let analysis = measure::analyze(kind, &elements);
        ModelNode {
            kind,
            elements,
            analysis,
        }
    }

    fn comment(
        &mut self,
        token: &rowan::SyntaxToken<crate::compiler::parse::cst::QueryLang>,
        slot: CommentSlot,
    ) -> Comment {
        let id = CommentId(self.next_comment_id);
        self.next_comment_id += 1;
        self.classifier.classify(id, slot, token)
    }
}
