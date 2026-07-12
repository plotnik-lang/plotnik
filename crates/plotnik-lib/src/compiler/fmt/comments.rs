use std::ops::Range;

use crate::compiler::parse::cst::{QueryLang, SyntaxKind, SyntaxToken};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct CommentId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct UnitId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct CommentSlot {
    pub owner: UnitId,
    pub element_index: u32,
}

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
    pub slot: CommentSlot,
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

pub(super) struct CommentClassifier<'q> {
    source: &'q str,
    significant_ranges: Vec<Range<usize>>,
    line_starts: Vec<usize>,
}

impl<'q> CommentClassifier<'q> {
    pub fn new(source: &'q str, significant_ranges: Vec<Range<usize>>) -> Self {
        let mut line_starts = vec![0];
        let bytes = source.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            match bytes[index] {
                b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                    index += 2;
                    line_starts.push(index);
                }
                b'\r' | b'\n' => {
                    index += 1;
                    line_starts.push(index);
                }
                _ => index += 1,
            }
        }
        Self {
            source,
            significant_ranges,
            line_starts,
        }
    }

    pub fn classify(
        &self,
        id: CommentId,
        slot: CommentSlot,
        token: &rowan::SyntaxToken<QueryLang>,
    ) -> Comment {
        let range = Range::<usize>::from(token.text_range());
        let line_index = self
            .line_starts
            .partition_point(|start| *start <= range.start)
            - 1;
        let line_start = self.line_starts[line_index];
        let line_end = self
            .line_starts
            .get(line_index + 1)
            .copied()
            .unwrap_or(self.source.len());
        let before_index = self
            .significant_ranges
            .partition_point(|significant| significant.end <= range.start);
        let code_before =
            before_index > 0 && self.significant_ranges[before_index - 1].start >= line_start;
        let after_index = self
            .significant_ranges
            .partition_point(|significant| significant.start < range.end);
        let code_after = self
            .significant_ranges
            .get(after_index)
            .is_some_and(|significant| significant.start < line_end);
        let multiline = token.text().contains(['\n', '\r']);
        let kind = if token.kind() == SyntaxKind::LineComment {
            CommentKind::Line
        } else {
            CommentKind::Block
        };
        let placement = if kind == CommentKind::Block && !multiline && code_before && code_after {
            CommentPlacement::InlineGap
        } else if code_before {
            CommentPlacement::Trailing
        } else {
            CommentPlacement::OwnLine
        };
        let normalized = if kind == CommentKind::Line {
            token.text().trim_end_matches([' ', '\t']).to_owned()
        } else {
            token.text().replace("\r\n", "\n").replace('\r', "\n")
        };
        Comment {
            id,
            kind,
            placement,
            text: CommentText::new(token.clone(), normalized),
            multiline,
            slot,
        }
    }
}
