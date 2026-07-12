use crate::compiler::parse::cst::SyntaxKind;

use super::comments::{Comment, CommentPlacement};
use super::contract;
use super::ir::{
    CaptureCount, Element, FormatFile, ModelNode, NodeKind, PrefixKind, SuffixKind, Width,
};
use super::tokens::{Atom, format_atoms};

const INDENT_WIDTH: usize = 2;
const LINE_WIDTH: usize = 80;

#[derive(Debug, Clone, Copy, Default)]
struct IndentLevel(usize);

impl IndentLevel {
    fn next(self) -> Self {
        Self(self.0 + 1)
    }

    fn width(self) -> usize {
        self.0 * INDENT_WIDTH
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Column(usize);

#[derive(Debug, Clone, Copy, Default)]
struct PendingSuffixes {
    width: Width,
    captures: CaptureCount,
}

impl PendingSuffixes {
    fn followed_by(self, suffixes: &SuffixChain<'_>) -> Self {
        Self {
            width: Width(self.width.0 + suffixes.width()),
            captures: CaptureCount(self.captures.0 + suffixes.capture_count()),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RenderContext {
    indent: IndentLevel,
    first_line_column: Column,
    pending_suffixes: PendingSuffixes,
}

impl RenderContext {
    fn root() -> Self {
        Self::default()
    }

    fn indented(self) -> Self {
        let indent = self.indent.next();
        Self {
            indent,
            first_line_column: Column(indent.width()),
            pending_suffixes: PendingSuffixes::default(),
        }
    }

    fn after_prefix(self, prefix: &str) -> Self {
        Self {
            first_line_column: Column(self.first_line_column.0 + prefix.chars().count()),
            ..self
        }
    }

    fn with_suffixes(self, suffixes: &SuffixChain<'_>) -> Self {
        Self {
            pending_suffixes: self.pending_suffixes.followed_by(suffixes),
            ..self
        }
    }
}

#[derive(Debug)]
enum Line {
    Generated { indent: IndentLevel, text: String },
    Verbatim { text: String },
}

impl Line {
    fn generated(indent: IndentLevel, text: String) -> Self {
        assert!(!text.contains(['\n', '\r']), "a generated line is one line");
        Self::Generated { indent, text }
    }

    fn verbatim(text: String) -> Self {
        assert!(!text.contains(['\n', '\r']), "a verbatim line is one line");
        Self::Verbatim { text }
    }

    fn text_mut(&mut self) -> &mut String {
        match self {
            Self::Generated { text, .. } | Self::Verbatim { text } => text,
        }
    }

    fn text(&self) -> &str {
        match self {
            Self::Generated { text, .. } | Self::Verbatim { text } => text,
        }
    }

    fn write_to(&self, output: &mut String) {
        match self {
            Self::Generated { indent, text } => {
                for _ in 0..indent.width() {
                    output.push(' ');
                }
                output.push_str(text);
            }
            Self::Verbatim { text } => output.push_str(text),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CodeSpan {
    first: usize,
    last: usize,
}

impl CodeSpan {
    fn inclusive(first: usize, last: usize, line_count: usize) -> Self {
        assert!(first <= last && last < line_count);
        Self { first, last }
    }

    fn shift(&mut self, offset: usize) {
        self.first += offset;
        self.last += offset;
    }
}

#[derive(Debug)]
struct Doc {
    lines: Vec<Line>,
    code: CodeSpan,
}

impl Doc {
    fn inline(indent: IndentLevel, text: String) -> Self {
        Self {
            lines: vec![Line::generated(indent, text)],
            code: CodeSpan::inclusive(0, 0, 1),
        }
    }

    fn from_lines(lines: Vec<Line>, code: CodeSpan) -> Self {
        assert!(!lines.is_empty(), "a document has at least one line");
        assert!(code.first <= code.last && code.last < lines.len());
        Self { lines, code }
    }

    fn is_multiline(&self) -> bool {
        self.lines.len() > 1
    }

    fn prepend_to_code(&mut self, prefix: &str) {
        self.lines[self.code.first].text_mut().insert_str(0, prefix);
    }

    fn prepend_lines(&mut self, mut lines: Vec<Line>) {
        if lines.is_empty() {
            return;
        }
        let offset = lines.len();
        lines.append(&mut self.lines);
        self.lines = lines;
        self.code.shift(offset);
    }

    fn append_lines(&mut self, lines: Vec<Line>) {
        self.lines.extend(lines);
    }

    fn write_to(&self, output: &mut String) {
        for (index, line) in self.lines.iter().enumerate() {
            if index > 0 {
                output.push('\n');
            }
            line.write_to(output);
        }
    }
}

enum FileItem {
    Shebang(Doc),
    Definition { doc: Doc, layout: DefinitionLayout },
    CommentBlock(Doc),
}

impl FileItem {
    fn doc(&self) -> &Doc {
        match self {
            Self::Shebang(doc) | Self::CommentBlock(doc) | Self::Definition { doc, .. } => doc,
        }
    }

    fn doc_mut(&mut self) -> &mut Doc {
        match self {
            Self::Shebang(doc) | Self::CommentBlock(doc) | Self::Definition { doc, .. } => doc,
        }
    }

    fn body_is_multiline(&self) -> bool {
        matches!(
            self,
            Self::Definition {
                layout: DefinitionLayout::Multiline,
                ..
            }
        )
    }
}

enum DefinitionLayout {
    SingleLine,
    Multiline,
}

struct SuffixSegment<'a> {
    kind: SuffixKind,
    text: String,
    forcing_comments: Vec<&'a Comment>,
}

impl SuffixSegment<'_> {
    fn width(&self) -> usize {
        self.separator().len() + self.text.chars().count()
    }

    fn separator(&self) -> &'static str {
        match self.kind {
            SuffixKind::Quantifier => "",
            SuffixKind::Capture => " ",
        }
    }

    fn append_to(&self, text: &mut String) {
        text.push_str(self.separator());
        text.push_str(&self.text);
    }
}

struct SuffixChain<'a> {
    base: &'a ModelNode,
    segments: Vec<SuffixSegment<'a>>,
}

struct PrefixSegment<'a> {
    text: String,
    forcing_comments: Vec<&'a Comment>,
}

struct PrefixChain<'a> {
    base: &'a ModelNode,
    segments: Vec<PrefixSegment<'a>>,
}

impl PrefixChain<'_> {
    fn text(&self) -> String {
        let capacity = self.segments.iter().map(|segment| segment.text.len()).sum();
        let mut text = String::with_capacity(capacity);
        for segment in &self.segments {
            text.push_str(&segment.text);
        }
        text
    }
}

impl SuffixChain<'_> {
    fn width(&self) -> usize {
        self.segments.iter().map(SuffixSegment::width).sum()
    }

    fn capture_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| segment.kind == SuffixKind::Capture)
            .count()
    }
}

pub(super) fn render(file: &FormatFile) -> String {
    let mut renderer = Renderer {
        emitted_comments: vec![false; file.comment_count],
        next_comment: 0,
    };
    let output = renderer.file(&file.root);
    assert_eq!(
        renderer.next_comment, file.comment_count,
        "the renderer emits every normalized comment exactly once"
    );
    assert!(
        renderer.emitted_comments.into_iter().all(|emitted| emitted),
        "the renderer emits every normalized comment"
    );
    contract::validate_rendered_comments(file, &output);
    output
}

struct Renderer {
    emitted_comments: Vec<bool>,
    next_comment: usize,
}

impl Renderer {
    fn file(&mut self, root: &ModelNode) -> String {
        let mut items: Vec<FileItem> = Vec::new();
        let mut pending_comments: Vec<&Comment> = Vec::new();

        for element in &root.elements {
            match element {
                Element::Node(node) if node.kind == NodeKind::Definition => {
                    let mut doc = self.node(node, RenderContext::root());
                    let layout = if doc.is_multiline() {
                        DefinitionLayout::Multiline
                    } else {
                        DefinitionLayout::SingleLine
                    };
                    doc.prepend_lines(
                        self.comment_block(&pending_comments, IndentLevel::default()),
                    );
                    pending_comments.clear();
                    items.push(FileItem::Definition { doc, layout });
                }
                Element::Comment(comment)
                    if comment.placement != CommentPlacement::OwnLine && !items.is_empty() =>
                {
                    self.record_comment(comment);
                    let item = items.last_mut().expect("checked nonempty");
                    let doc = item.doc_mut();
                    let mut parts = comment.normalized_lines();
                    let first = parts.next().expect("a comment has a first line");
                    doc.lines[doc.code.last].text_mut().push(' ');
                    doc.lines[doc.code.last].text_mut().push_str(first);
                    doc.append_lines(parts.map(|line| Line::verbatim(line.to_owned())).collect());
                }
                Element::Comment(comment) => pending_comments.push(comment),
                Element::Token(token) if token.kind == SyntaxKind::Shebang => {
                    let doc = Doc::inline(
                        IndentLevel::default(),
                        token.text().trim_end_matches(['\r', '\n']).to_owned(),
                    );
                    items.push(FileItem::Shebang(doc));
                }
                Element::Node(_) | Element::Token(_) => {}
            }
        }

        if !pending_comments.is_empty() {
            let lines = self.comment_block(&pending_comments, IndentLevel::default());
            let last = lines.len().saturating_sub(1);
            let code = CodeSpan::inclusive(0, last, lines.len());
            items.push(FileItem::CommentBlock(Doc::from_lines(lines, code)));
        }
        if items.is_empty() {
            return String::new();
        }

        let mut output = String::new();
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                output.push('\n');
                if items[index - 1].body_is_multiline() || item.body_is_multiline() {
                    output.push('\n');
                }
            }
            item.doc().write_to(&mut output);
        }
        output
    }

    fn node(&mut self, node: &ModelNode, context: RenderContext) -> Doc {
        match node.kind {
            NodeKind::Definition => self.definition(node),
            NodeKind::Group(_) => self.group(node, context),
            NodeKind::Prefix(kind) => self.prefixed(node, context, kind),
            NodeKind::Suffix(_) => self.suffixed(node, context),
            _ => self.atomic(node, context),
        }
    }

    fn definition(&mut self, node: &ModelNode) -> Doc {
        let body = node
            .children()
            .find(|child| child.kind.is_definition_body())
            .expect("parse-clean definition has a body");
        let body_index = node_index(node, body);
        let prefix = self.inline_elements(&node.elements[..body_index]);
        let comments = forcing_comments_deep(&node.elements[..body_index]);
        let mut doc = self.node(
            body,
            RenderContext::root().after_prefix(&format!("{prefix} ")),
        );
        doc.prepend_to_code(&format!("{prefix} "));
        self.attach_forcing_comments(&mut doc, &comments, IndentLevel::default());
        doc
    }

    fn prefixed(&mut self, node: &ModelNode, context: RenderContext, _kind: PrefixKind) -> Doc {
        let prefixes = self.collect_prefix_chain(node);
        let prefix = prefixes.text();
        let mut doc = self.node(prefixes.base, context.after_prefix(&prefix));
        doc.prepend_to_code(&prefix);
        for segment in &prefixes.segments {
            self.attach_forcing_comments(&mut doc, &segment.forcing_comments, context.indent);
        }
        doc
    }

    fn collect_prefix_chain<'a>(&mut self, node: &'a ModelNode) -> PrefixChain<'a> {
        let mut current = node;
        let mut segments = Vec::new();
        while let NodeKind::Prefix(kind) = current.kind {
            let child = current
                .children()
                .find(|child| child.kind.is_pattern())
                .expect("parse-clean prefixed item has a body");
            let child_index = node_index(current, child);
            let prefix_elements = &current.elements[..child_index];
            let mut text = self.inline_elements(prefix_elements);
            match kind {
                PrefixKind::Field => text.push(' '),
                PrefixKind::Branch if !text.is_empty() => text.push(' '),
                PrefixKind::Branch => {}
            }
            segments.push(PrefixSegment {
                text,
                forcing_comments: forcing_comments_deep(prefix_elements),
            });
            current = child;
        }
        PrefixChain {
            base: current,
            segments,
        }
    }

    fn suffixed(&mut self, node: &ModelNode, context: RenderContext) -> Doc {
        let suffixes = self.collect_suffix_chain(node);
        let mut doc = self.node(suffixes.base, context.with_suffixes(&suffixes));
        for segment in &suffixes.segments {
            segment.append_to(doc.lines[doc.code.last].text_mut());
            self.attach_forcing_comments(&mut doc, &segment.forcing_comments, context.indent);
        }
        doc
    }

    fn collect_suffix_chain<'a>(&mut self, node: &'a ModelNode) -> SuffixChain<'a> {
        let mut current = node;
        let mut segments = Vec::new();
        while let NodeKind::Suffix(kind) = current.kind {
            let inner = current
                .children()
                .find(|child| child.kind.is_pattern())
                .expect("parse-clean suffix has an inner pattern");
            let inner_index = node_index(current, inner);
            let suffix_elements = &current.elements[inner_index + 1..];
            segments.push(SuffixSegment {
                kind,
                text: self.inline_elements(suffix_elements),
                forcing_comments: forcing_comments_deep(suffix_elements),
            });
            current = inner;
        }
        segments.reverse();
        SuffixChain {
            base: current,
            segments,
        }
    }

    fn group(&mut self, node: &ModelNode, context: RenderContext) -> Doc {
        let first_item = node.children().find(|child| node.is_group_item(child));
        let has_items = first_item.is_some();
        let width_overflow = has_items
            && context.first_line_column.0
                + node.analysis.inline.width.0
                + context.pending_suffixes.width.0
                > LINE_WIDTH;
        let capture_dense =
            node.analysis.inline.captures.0 + context.pending_suffixes.captures.0 >= 2;
        let must_break = node.analysis.must_break || capture_dense || width_overflow;
        if !must_break || !has_items && !node.analysis.inline.has_hardline {
            return self.inline(node, context);
        }
        self.broken_group(node, context, first_item)
    }

    fn broken_group(
        &mut self,
        node: &ModelNode,
        context: RenderContext,
        first_item: Option<&ModelNode>,
    ) -> Doc {
        let first_index = first_item.map_or_else(
            || {
                node.elements
                    .iter()
                    .position(|element| element.comment().is_some_and(Comment::forces_line))
                    .unwrap_or_else(|| closer_index(node))
            },
            |item| node_index(node, item),
        );
        let head = self.inline_elements(&node.elements[..first_index]);
        let mut lines = vec![Line::generated(context.indent, head)];
        let mut last_code = 0;

        for comment in forcing_comments_deep(&node.elements[..first_index]) {
            self.emit_comment_into(&mut lines, &mut last_code, comment, context.indent.next());
        }

        for element in &node.elements[first_index..closer_index(node)] {
            match element {
                Element::Node(child) if node.is_group_item(child) => {
                    let child_doc = self.node(child, context.indented());
                    let offset = lines.len();
                    last_code = offset + child_doc.code.last;
                    lines.extend(child_doc.lines);
                }
                Element::Comment(comment) => {
                    self.emit_comment_into(
                        &mut lines,
                        &mut last_code,
                        comment,
                        context.indent.next(),
                    );
                }
                Element::Node(_) | Element::Token(_) => {}
            }
        }

        let closer = node
            .elements
            .iter()
            .rev()
            .find_map(|element| match element {
                Element::Token(token)
                    if token.kind
                        == node
                            .group_kind()
                            .expect("broken group has group semantics")
                            .close_token() =>
                {
                    Some(token.text().to_owned())
                }
                _ => None,
            })
            .expect("parse-clean group has a closer");
        last_code = lines.len();
        lines.push(Line::generated(context.indent, closer));
        let code = CodeSpan::inclusive(0, last_code, lines.len());
        Doc::from_lines(lines, code)
    }

    fn atomic(&mut self, node: &ModelNode, context: RenderContext) -> Doc {
        if !node.analysis.inline.has_hardline {
            return self.inline(node, context);
        }
        let text = self.inline_elements(&node.elements);
        let mut doc = Doc::inline(context.indent, text);
        let comments = forcing_comments_deep(&node.elements);
        self.attach_forcing_comments(&mut doc, &comments, context.indent);
        doc
    }

    fn inline(&mut self, node: &ModelNode, context: RenderContext) -> Doc {
        node.for_each_descendant_comment(&mut |comment| self.record_comment(comment));
        let text = self.inline_elements_without_recording(&node.elements);
        Doc::inline(context.indent, text)
    }

    fn inline_elements(&mut self, elements: &[Element]) -> String {
        let mut atoms = Vec::new();
        collect_atoms(elements, &mut atoms);
        for atom in &atoms {
            if let Atom::Comment(comment) = atom {
                self.record_comment(comment);
            }
        }
        format_atoms(&atoms)
    }

    fn inline_elements_without_recording(&self, elements: &[Element]) -> String {
        let mut atoms = Vec::new();
        collect_atoms(elements, &mut atoms);
        format_atoms(&atoms)
    }

    fn attach_forcing_comments(
        &mut self,
        doc: &mut Doc,
        comments: &[&Comment],
        indent: IndentLevel,
    ) {
        let first_trailing = comments
            .iter()
            .position(|comment| comment.placement != CommentPlacement::OwnLine)
            .unwrap_or(comments.len());
        doc.prepend_lines(self.comment_block(&comments[..first_trailing], indent));

        let mut append_at = doc.code.last;
        for comment in &comments[first_trailing..] {
            if comment.placement == CommentPlacement::OwnLine {
                let lines = self.comment_block(&[*comment], indent);
                append_at = doc.lines.len() + lines.len() - 1;
                doc.append_lines(lines);
                continue;
            }
            let lines = self.comment_after_code(comment, indent);
            let Some(first) = lines.first() else {
                continue;
            };
            doc.lines[append_at].text_mut().push(' ');
            doc.lines[append_at].text_mut().push_str(first.text());
            if lines.len() > 1 {
                append_at = doc.lines.len() + lines.len() - 2;
                doc.append_lines(lines.into_iter().skip(1).collect());
            }
        }
    }

    fn comment_block(&mut self, comments: &[&Comment], indent: IndentLevel) -> Vec<Line> {
        let mut lines = Vec::new();
        for comment in comments {
            self.record_comment(comment);
            let mut parts = comment.normalized_lines();
            let first = parts.next().expect("a comment has a first line");
            lines.push(Line::generated(indent, first.to_owned()));
            lines.extend(parts.map(|line| Line::verbatim(line.to_owned())));
        }
        lines
    }

    fn comment_after_code(&mut self, comment: &Comment, indent: IndentLevel) -> Vec<Line> {
        self.record_comment(comment);
        let mut parts = comment.normalized_lines();
        let first = parts.next().expect("a comment has a first line");
        let mut lines = vec![Line::generated(indent, first.to_owned())];
        lines.extend(parts.map(|line| Line::verbatim(line.to_owned())));
        lines
    }

    fn emit_comment_into(
        &mut self,
        lines: &mut Vec<Line>,
        last_code: &mut usize,
        comment: &Comment,
        indent: IndentLevel,
    ) {
        self.record_comment(comment);
        let mut parts = comment.normalized_lines();
        let first = parts.next().expect("a comment has a first line");
        if comment.placement != CommentPlacement::OwnLine {
            lines[*last_code].text_mut().push(' ');
            lines[*last_code].text_mut().push_str(first);
        } else {
            lines.push(Line::generated(indent, first.to_owned()));
        }
        lines.extend(parts.map(|line| Line::verbatim(line.to_owned())));
    }

    fn record_comment(&mut self, comment: &Comment) {
        let id = comment.id.0 as usize;
        assert!(!self.emitted_comments[id], "each CommentId is emitted once");
        self.emitted_comments[id] = true;
        self.next_comment += 1;
    }
}

fn forcing_comments_deep(elements: &[Element]) -> Vec<&Comment> {
    let mut comments = Vec::new();
    collect_forcing_comments(elements, &mut comments);
    comments
}

fn collect_forcing_comments<'a>(elements: &'a [Element], comments: &mut Vec<&'a Comment>) {
    for element in elements {
        match element {
            Element::Node(node) => collect_forcing_comments(&node.elements, comments),
            Element::Comment(comment) if comment.forces_line() => comments.push(comment),
            Element::Token(_) | Element::Comment(_) => {}
        }
    }
}

fn collect_atoms<'a>(elements: &'a [Element], atoms: &mut Vec<Atom<'a>>) {
    for element in elements {
        match element {
            Element::Node(node) => collect_atoms(&node.elements, atoms),
            Element::Token(token) => atoms.push(Atom::Token(token)),
            Element::Comment(comment) if !comment.forces_line() => {
                atoms.push(Atom::Comment(comment));
            }
            Element::Comment(_) => {}
        }
    }
}

fn node_index(parent: &ModelNode, child: &ModelNode) -> usize {
    parent
        .elements
        .iter()
        .position(|element| {
            element
                .node()
                .is_some_and(|candidate| std::ptr::eq(candidate, child))
        })
        .expect("child belongs to parent")
}

fn closer_index(node: &ModelNode) -> usize {
    let close = node
        .group_kind()
        .expect("only groups have closing delimiters")
        .close_token();
    node.elements
        .iter()
        .rposition(|element| matches!(element, Element::Token(token) if token.kind == close))
        .expect("parse-clean group has a closer")
}
