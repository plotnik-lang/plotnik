use rowan::NodeOrToken;

use crate::compiler::parse::Root;
use crate::compiler::parse::cst::{SyntaxKind, SyntaxNode};

use super::comments::{Comment, CommentClassifier};
use super::contract;
use super::ir::{Element, FormatFile, ModelNode, NodeKind, NodeLayout, Token, WorkCounter};
use super::measure;

pub(super) fn normalize(source: &str, root: &Root) -> FormatFile {
    let mut builder = Builder {
        classifier: CommentClassifier::new(source, root.syntax()),
        work: WorkCounter::default(),
    };
    builder.work.add(builder.classifier.token_count());
    let root = builder.node(root.syntax());
    assert!(
        builder.classifier.is_empty(),
        "normalization consumes every classified comment"
    );
    let file = FormatFile {
        root,
        comment_count: builder.classifier.comment_count(),
        source_len: source.len(),
        normalization_work: builder.work.value(),
    };
    contract::validate_model(&file);
    file
}

struct Builder {
    classifier: CommentClassifier,
    work: WorkCounter,
}

impl Builder {
    fn node(&mut self, syntax: &SyntaxNode) -> ModelNode {
        let elements: Vec<Element> = syntax
            .children_with_tokens()
            .filter_map(|element| match element {
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
                    Some(Element::Comment(self.comment(&token)))
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
        let layout = NodeLayout::for_node(kind, &elements);
        let analysis = measure::analyze(&layout, &elements);
        self.work.add(1 + elements.len() * 2);
        ModelNode {
            kind,
            elements,
            layout,
            analysis,
        }
    }

    fn comment(&mut self, token: &crate::compiler::parse::cst::SyntaxToken) -> Comment {
        self.classifier.take(token)
    }
}
