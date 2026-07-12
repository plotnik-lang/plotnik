use std::collections::VecDeque;
use std::ops::Range;

use rowan::NodeOrToken;

use crate::compiler::parse::cst::{SyntaxKind, SyntaxNode, SyntaxToken};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct CommentId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommentPlacement {
    OwnLine,
    InlineGap,
    Trailing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommentKind {
    Line,
    Block,
}

#[derive(Debug, Clone)]
pub(super) struct Comment {
    pub id: CommentId,
    pub kind: CommentKind,
    pub placement: CommentPlacement,
    text: CommentText,
    pub multiline: bool,
}

impl Comment {
    pub fn forces_line(&self) -> bool {
        self.kind == CommentKind::Line
            || self.multiline
            || self.placement != CommentPlacement::InlineGap
    }

    pub fn normalized_lines(&self) -> impl Iterator<Item = &str> {
        self.text().split('\n')
    }

    pub fn text(&self) -> &str {
        self.text.as_str()
    }
}

#[derive(Debug, Clone)]
struct CommentText {
    syntax: SyntaxToken,
    normalized: Option<String>,
}

impl CommentText {
    fn new(syntax: SyntaxToken, normalized: String) -> Self {
        let normalized = (normalized != syntax.text()).then_some(normalized);
        Self { syntax, normalized }
    }

    fn as_str(&self) -> &str {
        self.normalized
            .as_deref()
            .unwrap_or_else(|| self.syntax.text())
    }
}

#[derive(Default)]
struct LineFacts {
    first_code_start: Option<usize>,
    last_code_end: Option<usize>,
}

impl LineFacts {
    fn record_code(&mut self, range: &Range<usize>) {
        self.first_code_start.get_or_insert(range.start);
        self.last_code_end = Some(range.end);
    }

    fn has_code_before(&self, offset: usize) -> bool {
        self.first_code_start.is_some_and(|start| start < offset)
    }

    fn has_code_after(&self, offset: usize) -> bool {
        self.last_code_end.is_some_and(|end| end > offset)
    }
}

struct UnclassifiedComment {
    syntax: SyntaxToken,
    line: usize,
}

pub(super) struct CommentClassifier {
    comments: VecDeque<Comment>,
    comment_count: usize,
    token_count: usize,
}

impl CommentClassifier {
    pub fn new(source: &str, root: &SyntaxNode) -> Self {
        let line_starts = line_starts(source);
        let mut line_facts = (0..line_starts.len())
            .map(|_| LineFacts::default())
            .collect::<Vec<_>>();
        let mut comments = Vec::new();
        let mut line = 0;
        let mut token_count = 0;

        for token in root
            .descendants_with_tokens()
            .filter_map(NodeOrToken::into_token)
        {
            token_count += 1;
            let range = Range::<usize>::from(token.text_range());
            while line_starts
                .get(line + 1)
                .is_some_and(|start| *start <= range.start)
            {
                line += 1;
            }
            if matches!(
                token.kind(),
                SyntaxKind::LineComment | SyntaxKind::BlockComment
            ) {
                comments.push(UnclassifiedComment {
                    syntax: token,
                    line,
                });
                continue;
            }
            if !token.kind().is_trivia() {
                line_facts[line].record_code(&range);
            }
        }

        let comments = comments
            .into_iter()
            .enumerate()
            .map(|(id, comment)| {
                let id = u32::try_from(id).expect("comment count fits in u32");
                classify(CommentId(id), comment, &line_facts)
            })
            .collect::<VecDeque<_>>();
        let comment_count = comments.len();
        Self {
            comments,
            comment_count,
            token_count,
        }
    }

    pub fn take(&mut self, token: &SyntaxToken) -> Comment {
        let comment = self
            .comments
            .pop_front()
            .expect("every CST comment was classified");
        assert_eq!(
            comment.text.syntax.text_range(),
            token.text_range(),
            "normalization consumes comments in source order"
        );
        comment
    }

    pub fn comment_count(&self) -> usize {
        self.comment_count
    }

    pub fn is_empty(&self) -> bool {
        self.comments.is_empty()
    }

    pub fn token_count(&self) -> usize {
        self.token_count
    }
}

fn classify(id: CommentId, comment: UnclassifiedComment, lines: &[LineFacts]) -> Comment {
    let range = Range::<usize>::from(comment.syntax.text_range());
    let multiline = comment.syntax.text().contains(['\n', '\r']);
    let kind = if comment.syntax.kind() == SyntaxKind::LineComment {
        CommentKind::Line
    } else {
        CommentKind::Block
    };
    let code_before = lines[comment.line].has_code_before(range.start);
    let code_after = lines[comment.line].has_code_after(range.end);
    let placement = if kind == CommentKind::Block && !multiline && code_before && code_after {
        CommentPlacement::InlineGap
    } else if code_before {
        CommentPlacement::Trailing
    } else {
        CommentPlacement::OwnLine
    };
    let normalized = if kind == CommentKind::Line {
        comment
            .syntax
            .text()
            .trim_end_matches([' ', '\t'])
            .to_owned()
    } else {
        comment
            .syntax
            .text()
            .replace("\r\n", "\n")
            .replace('\r', "\n")
    };
    Comment {
        id,
        kind,
        placement,
        text: CommentText::new(comment.syntax, normalized),
        multiline,
    }
}

fn line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => index += 2,
            b'\r' | b'\n' => index += 1,
            _ => {
                index += 1;
                continue;
            }
        }
        starts.push(index);
    }
    starts
}
