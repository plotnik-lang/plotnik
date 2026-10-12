use super::comments::{Comment, CommentKind, CommentPlacement};
use super::contract;
use super::ir::{
    Element, FilePart, FormatFile, GroupLayout, GroupPart, LandmarkCount, ModelNode, NodeKind,
    NodeLayout, PrefixKind, SuffixKind, Token, Width, WorkCounter,
};
use super::tokens::{Atom, format_atoms};

const INDENT_WIDTH: usize = 2;
const LINE_WIDTH: usize = 80;
const INLINE_LANDMARK_BUDGET: usize = 3;

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
struct PendingAffixes {
    suffix_width: Width,
    landmarks: LandmarkCount,
}

impl PendingAffixes {
    fn followed_by(self, prefixes: &[AffixSegment<'_>], suffixes: &[AffixSegment<'_>]) -> Self {
        let suffix_width: usize = suffixes.iter().map(AffixSegment::suffix_width).sum();
        let landmarks = prefixes
            .iter()
            .chain(suffixes)
            .map(|segment| segment.landmarks.0)
            .sum::<usize>();
        Self {
            suffix_width: Width(self.suffix_width.0 + suffix_width),
            landmarks: LandmarkCount(self.landmarks.0.saturating_add(landmarks)),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RenderContext {
    indent: IndentLevel,
    first_line_column: Column,
    pending_affixes: PendingAffixes,
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
            pending_affixes: PendingAffixes::default(),
        }
    }

    fn after_width(self, width: usize) -> Self {
        Self {
            first_line_column: Column(self.first_line_column.0 + width),
            ..self
        }
    }

    fn with_affixes(self, prefixes: &[AffixSegment<'_>], suffixes: &[AffixSegment<'_>]) -> Self {
        Self {
            pending_affixes: self.pending_affixes.followed_by(prefixes, suffixes),
            ..self
        }
    }
}

struct Output {
    text: String,
    at_line_start: bool,
    line_comment_open: bool,
    work: WorkCounter,
}

impl Output {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            text: String::with_capacity(capacity),
            at_line_start: true,
            line_comment_open: false,
            work: WorkCounter::default(),
        }
    }

    fn ensure_indent(&mut self, indent: IndentLevel) {
        if !self.at_line_start {
            return;
        }
        for _ in 0..indent.width() {
            self.text.push(' ');
        }
        self.work.add(indent.width());
        self.at_line_start = false;
    }

    fn append(&mut self, text: &str) {
        assert!(
            !self.at_line_start,
            "indent is selected before text emission"
        );
        self.text.push_str(text);
        self.work.add(text.len());
    }

    fn append_space(&mut self) {
        self.append(" ");
    }

    fn newline(&mut self, indent: IndentLevel) {
        self.text.push('\n');
        self.work.add(1);
        self.at_line_start = true;
        self.line_comment_open = false;
        self.ensure_indent(indent);
    }

    fn newline_verbatim(&mut self, text: &str) {
        self.text.push('\n');
        self.text.push_str(text);
        self.work.add(1 + text.len());
        self.at_line_start = false;
        self.line_comment_open = false;
    }

    fn separate_file_items(&mut self, blank: bool) {
        assert!(
            !self.at_line_start,
            "a file separator follows a rendered item"
        );
        self.text.push('\n');
        self.work.add(1);
        if blank {
            self.text.push('\n');
            self.work.add(1);
        }
        self.at_line_start = true;
        self.line_comment_open = false;
    }

    fn finish(self) -> (String, usize) {
        (self.text, self.work.value())
    }
}

struct AffixSegment<'a> {
    text: String,
    inline_comments: Vec<&'a Comment>,
    boundary_comments: Vec<&'a Comment>,
    landmarks: LandmarkCount,
    suffix_kind: Option<SuffixKind>,
}

impl AffixSegment<'_> {
    fn suffix_separator(&self) -> &'static str {
        match self.suffix_kind {
            Some(SuffixKind::Capture) => " ",
            Some(SuffixKind::Quantifier) | None => "",
        }
    }

    fn suffix_width(&self) -> usize {
        self.suffix_separator().len() + self.text.chars().count()
    }
}

struct NodePlan<'a> {
    core: &'a ModelNode,
    prefixes: Vec<AffixSegment<'a>>,
    suffixes: Vec<AffixSegment<'a>>,
    comments: CommentBoundaries<'a>,
}

impl NodePlan<'_> {
    fn prefix_width(&self) -> usize {
        self.prefixes
            .iter()
            .map(|segment| segment.text.chars().count())
            .sum()
    }
}

struct CommentBoundaries<'a> {
    before: Vec<&'a Comment>,
    after: Vec<&'a Comment>,
}

impl<'a> CommentBoundaries<'a> {
    fn from_comments(mut comments: Vec<&'a Comment>) -> Self {
        comments.sort_by_key(|comment| comment.id.0);
        let first_after = comments
            .iter()
            .position(|comment| comment.placement != CommentPlacement::OwnLine)
            .unwrap_or(comments.len());
        let after = comments.split_off(first_after);
        Self {
            before: comments,
            after,
        }
    }

    fn is_multiline(&self) -> bool {
        !self.before.is_empty() || comments_after_are_multiline(&self.after)
    }
}

fn comments_after_are_multiline(comments: &[&Comment]) -> bool {
    comments
        .iter()
        .any(|comment| comment.multiline || comment.placement == CommentPlacement::OwnLine)
        || comments
            .windows(2)
            .any(|pair| pair[0].kind == CommentKind::Line)
}

enum FileBody<'a> {
    Shebang(&'a Token),
    Definition(DefinitionPlan<'a>),
    CommentBlock(Vec<&'a Comment>),
}

struct DefinitionPlan<'a> {
    prefix_text: String,
    prefix_inline_comments: Vec<&'a Comment>,
    prefix_width: usize,
    body: NodePlan<'a>,
    comments: CommentBoundaries<'a>,
    multiline: bool,
}

struct FilePlan<'a> {
    body: FileBody<'a>,
    leading_comments: Vec<&'a Comment>,
    trailing_comments: Vec<&'a Comment>,
}

pub(super) struct RenderedFile {
    pub output: String,
    pub work: usize,
}

pub(super) fn render(file: &FormatFile) -> RenderedFile {
    let mut renderer = Renderer {
        emitted_comments: vec![false; file.comment_count],
        emitted_count: 0,
        work: WorkCounter::default(),
    };
    let mut output = Output::with_capacity(file.source_len);
    renderer.file(&file.root, &mut output);
    assert_eq!(
        renderer.emitted_count, file.comment_count,
        "the renderer emits every normalized comment exactly once"
    );
    assert!(
        renderer.emitted_comments.into_iter().all(|emitted| emitted),
        "the renderer emits every normalized comment"
    );
    let (output, output_work) = output.finish();
    contract::validate_rendered_comments(file, &output);
    RenderedFile {
        output,
        work: renderer.work.value() + output_work,
    }
}

struct Renderer {
    emitted_comments: Vec<bool>,
    emitted_count: usize,
    work: WorkCounter,
}

impl Renderer {
    fn file(&mut self, root: &ModelNode, output: &mut Output) {
        let plans = self.file_plans(root);
        let multiline = plans
            .iter()
            .map(Self::file_plan_is_multiline)
            .collect::<Vec<_>>();

        for (index, plan) in plans.iter().enumerate() {
            if index > 0 {
                output.separate_file_items(multiline[index - 1] || multiline[index]);
            }
            self.render_file_plan(plan, output);
        }
    }

    fn file_plans<'a>(&mut self, root: &'a ModelNode) -> Vec<FilePlan<'a>> {
        let NodeLayout::Root { parts } = &root.layout else {
            unreachable!("the format file root has root layout")
        };
        let mut plans: Vec<FilePlan<'a>> = Vec::new();
        let mut pending_comments = Vec::new();
        self.work.add(parts.len());

        for part in parts {
            match *part {
                FilePart::Definition(index) => {
                    let definition = self.definition_plan(root.node_at(index));
                    plans.push(FilePlan {
                        body: FileBody::Definition(definition),
                        leading_comments: std::mem::take(&mut pending_comments),
                        trailing_comments: Vec::new(),
                    });
                }
                FilePart::Comment(index)
                    if root.comment_at(index).placement != CommentPlacement::OwnLine
                        && !plans.is_empty() =>
                {
                    plans
                        .last_mut()
                        .expect("checked nonempty")
                        .trailing_comments
                        .push(root.comment_at(index));
                }
                FilePart::Comment(index) => pending_comments.push(root.comment_at(index)),
                FilePart::Shebang(index) => {
                    let Element::Token(token) = &root.elements[index] else {
                        unreachable!("shebang file part points at a token")
                    };
                    plans.push(FilePlan {
                        body: FileBody::Shebang(token),
                        leading_comments: Vec::new(),
                        trailing_comments: Vec::new(),
                    });
                }
            }
        }
        if !pending_comments.is_empty() {
            plans.push(FilePlan {
                body: FileBody::CommentBlock(pending_comments),
                leading_comments: Vec::new(),
                trailing_comments: Vec::new(),
            });
        }
        plans
    }

    fn file_plan_is_multiline(plan: &FilePlan<'_>) -> bool {
        let body_is_multiline = match &plan.body {
            FileBody::Definition(plan) => plan.multiline,
            FileBody::Shebang(_) | FileBody::CommentBlock(_) => false,
        };
        body_is_multiline || comments_after_are_multiline(&plan.trailing_comments)
    }

    fn render_file_plan(&mut self, plan: &FilePlan<'_>, output: &mut Output) {
        if !plan.leading_comments.is_empty() {
            self.emit_comment_block(&plan.leading_comments, IndentLevel::default(), output);
            output.newline(IndentLevel::default());
        }
        match &plan.body {
            FileBody::Shebang(token) => {
                output.ensure_indent(IndentLevel::default());
                output.append(token.text().trim_end_matches(['\r', '\n']));
            }
            FileBody::Definition(definition) => self.render_definition(definition, output),
            FileBody::CommentBlock(comments) => {
                self.emit_comment_block(comments, IndentLevel::default(), output);
            }
        }
        self.emit_after(&plan.trailing_comments, IndentLevel::default(), output);
    }

    fn definition_plan<'a>(&mut self, node: &'a ModelNode) -> DefinitionPlan<'a> {
        let NodeLayout::Definition { prefix, body } = node.layout else {
            unreachable!("definition node has definition layout")
        };
        let prefix_elements = node.fragment(prefix);
        let (prefix_inline_comments, prefix_boundary_comments) =
            self.split_comments(prefix_elements);
        let prefix_text = self.inline_text(prefix_elements);
        let prefix_width = prefix_text.chars().count();
        let body_plan = self.node_plan(node.node_at(body));
        let multiline = !prefix_boundary_comments.is_empty()
            || self.plan_core_is_multiline(
                &body_plan,
                RenderContext::root().after_width(prefix_width + 1),
            );
        DefinitionPlan {
            prefix_text,
            prefix_inline_comments,
            prefix_width,
            body: body_plan,
            comments: CommentBoundaries {
                before: Vec::new(),
                after: prefix_boundary_comments,
            },
            multiline,
        }
    }

    fn render_definition(&mut self, plan: &DefinitionPlan<'_>, output: &mut Output) {
        output.ensure_indent(IndentLevel::default());
        self.record_comments(&plan.prefix_inline_comments);
        output.append(&plan.prefix_text);
        let body_has_leading_comments = !plan.body.comments.before.is_empty();
        let boundary_before_body = !plan.comments.after.is_empty() || body_has_leading_comments;
        if plan.comments.after.is_empty() {
            if !body_has_leading_comments {
                output.append_space();
            }
        } else {
            self.emit_after(&plan.comments.after, IndentLevel::default(), output);
        }
        if body_has_leading_comments {
            self.emit_leading(&plan.body.comments.before, IndentLevel::default(), output);
        } else if boundary_before_body {
            output.newline(IndentLevel::default());
        }
        let context = if boundary_before_body {
            RenderContext::root()
        } else {
            RenderContext::root().after_width(plan.prefix_width + 1)
        };
        self.render_plan_core(&plan.body, context, output);
        self.emit_after(&plan.body.comments.after, IndentLevel::default(), output);
    }

    fn render_node(&mut self, node: &ModelNode, context: RenderContext, output: &mut Output) {
        let plan = self.node_plan(node);
        self.emit_leading(&plan.comments.before, context.indent, output);
        self.render_plan_core(&plan, context, output);
        self.emit_after(&plan.comments.after, context.indent, output);
    }

    fn node_plan<'a>(&mut self, node: &'a ModelNode) -> NodePlan<'a> {
        let mut current = node;
        let mut prefixes = Vec::new();
        let mut suffixes = Vec::new();
        let mut before_comments = Vec::new();

        loop {
            self.work.add(1);
            match (current.kind, &current.layout) {
                (NodeKind::Prefix(kind), NodeLayout::Prefix { prefix, body }) => {
                    let elements = current.fragment(*prefix);
                    let body_node = current.node_at(*body);
                    let landmarks = LandmarkCount(
                        current
                            .analysis
                            .inline
                            .landmarks
                            .0
                            .saturating_sub(body_node.analysis.inline.landmarks.0),
                    );
                    let mut text = self.inline_text(elements);
                    match kind {
                        PrefixKind::Field => text.push(' '),
                        PrefixKind::Alternative { .. } if !text.is_empty() => text.push(' '),
                        PrefixKind::Alternative { .. } => {}
                    }
                    let (inline_comments, boundary_comments) = self.split_comments(elements);
                    before_comments.extend(boundary_comments.iter().copied());
                    prefixes.push(AffixSegment {
                        text,
                        inline_comments,
                        boundary_comments,
                        landmarks,
                        suffix_kind: None,
                    });
                    current = body_node;
                }
                (NodeKind::Suffix(kind), NodeLayout::Suffix { body, suffix }) => {
                    let elements = current.fragment(*suffix);
                    let body_node = current.node_at(*body);
                    let landmarks = LandmarkCount(
                        current
                            .analysis
                            .inline
                            .landmarks
                            .0
                            .saturating_sub(body_node.analysis.inline.landmarks.0),
                    );
                    let (inline_comments, boundary_comments) = self.split_comments(elements);
                    suffixes.push(AffixSegment {
                        text: self.inline_text(elements),
                        inline_comments,
                        boundary_comments,
                        landmarks,
                        suffix_kind: Some(kind),
                    });
                    current = body_node;
                }
                _ => break,
            }
        }
        suffixes.reverse();
        let atomic_comments = if matches!(current.layout, NodeLayout::Atomic) {
            CommentBoundaries::from_comments(self.boundary_comments(&current.elements))
        } else {
            CommentBoundaries::from_comments(Vec::new())
        };
        before_comments.extend(atomic_comments.before);
        let mut after_comments = atomic_comments.after;
        for suffix in &suffixes {
            after_comments.extend(suffix.boundary_comments.iter().copied());
        }
        NodePlan {
            core: current,
            prefixes,
            suffixes,
            comments: CommentBoundaries {
                before: before_comments,
                after: after_comments,
            },
        }
    }

    fn plan_core_is_multiline(&self, plan: &NodePlan<'_>, context: RenderContext) -> bool {
        if plan.comments.is_multiline() {
            return true;
        }
        let context = context
            .after_width(plan.prefix_width())
            .with_affixes(&plan.prefixes, &plan.suffixes);
        match &plan.core.layout {
            NodeLayout::Group(group) => self.group_must_break(plan.core, group, context),
            NodeLayout::Atomic => false,
            NodeLayout::Root { .. }
            | NodeLayout::Definition { .. }
            | NodeLayout::Prefix { .. }
            | NodeLayout::Suffix { .. } => {
                unreachable!("node plans unwrap wrappers to a group or atomic core")
            }
        }
    }

    fn render_plan_core(
        &mut self,
        plan: &NodePlan<'_>,
        context: RenderContext,
        output: &mut Output,
    ) {
        output.ensure_indent(context.indent);
        for prefix in &plan.prefixes {
            self.record_comments(&prefix.inline_comments);
            output.append(&prefix.text);
        }
        let context = context
            .after_width(plan.prefix_width())
            .with_affixes(&plan.prefixes, &plan.suffixes);
        match &plan.core.layout {
            NodeLayout::Group(group) => self.render_group(plan.core, group, context, output),
            NodeLayout::Atomic => {
                self.record_inline_comments(&plan.core.elements);
                output.append(&self.inline_text(&plan.core.elements));
            }
            NodeLayout::Root { .. }
            | NodeLayout::Definition { .. }
            | NodeLayout::Prefix { .. }
            | NodeLayout::Suffix { .. } => {
                unreachable!("node plans unwrap wrappers to a group or atomic core")
            }
        }
        for suffix in &plan.suffixes {
            self.record_comments(&suffix.inline_comments);
            output.append(suffix.suffix_separator());
            output.append(&suffix.text);
        }
    }

    fn render_group(
        &mut self,
        node: &ModelNode,
        group: &GroupLayout,
        context: RenderContext,
        output: &mut Output,
    ) {
        if !self.group_must_break(node, group, context) {
            node.for_each_descendant_comment(&mut |comment| self.record_comment(comment));
            output.append(&self.inline_text(&node.elements));
            return;
        }

        let head_elements = node.fragment(group.head);
        let (inline_comments, boundary_comments) = self.split_comments(head_elements);
        self.record_comments(&inline_comments);
        output.append(&self.inline_text(head_elements));
        self.emit_after(&boundary_comments, context.indent.next(), output);
        for part in &group.parts {
            self.work.add(1);
            match *part {
                GroupPart::Item(index) => {
                    output.newline(context.indent.next());
                    self.render_node(node.node_at(index), context.indented(), output);
                }
                GroupPart::Comment(index) => {
                    self.emit_boundary_comment(
                        node.comment_at(index),
                        context.indent.next(),
                        output,
                    );
                }
            }
        }
        let Element::Token(closer) = &node.elements[group.closer] else {
            unreachable!("group closer boundary points at a token")
        };
        output.newline(context.indent);
        output.append(closer.text());
    }

    fn group_must_break(
        &self,
        node: &ModelNode,
        group: &GroupLayout,
        context: RenderContext,
    ) -> bool {
        let has_items = group.has_items();
        let width_overflow = has_items
            && context.first_line_column.0
                + node.analysis.inline.width.0
                + context.pending_affixes.suffix_width.0
                > LINE_WIDTH;
        let semantic_density = node
            .analysis
            .inline
            .landmarks
            .0
            .saturating_add(context.pending_affixes.landmarks.0)
            > INLINE_LANDMARK_BUDGET;
        let must_break = node.analysis.must_break || semantic_density || width_overflow;
        must_break && (has_items || node.analysis.inline.has_hardline)
    }

    fn emit_leading(&mut self, comments: &[&Comment], indent: IndentLevel, output: &mut Output) {
        if comments.is_empty() {
            return;
        }
        for (index, comment) in comments.iter().enumerate() {
            if index > 0 || !output.at_line_start {
                output.newline(indent);
            } else {
                output.ensure_indent(indent);
            }
            self.record_comment(comment);
            self.write_comment_text(comment, output);
        }
        output.newline(indent);
    }

    fn emit_after(&mut self, comments: &[&Comment], indent: IndentLevel, output: &mut Output) {
        for comment in comments {
            if comment.placement == CommentPlacement::OwnLine || output.line_comment_open {
                output.newline(indent);
            } else {
                output.append_space();
            }
            self.record_comment(comment);
            self.write_comment_text(comment, output);
        }
    }

    fn emit_comment_block(
        &mut self,
        comments: &[&Comment],
        indent: IndentLevel,
        output: &mut Output,
    ) {
        for (index, comment) in comments.iter().enumerate() {
            if index > 0 {
                output.newline(indent);
            } else {
                output.ensure_indent(indent);
            }
            self.record_comment(comment);
            self.write_comment_text(comment, output);
        }
    }

    fn emit_boundary_comment(
        &mut self,
        comment: &Comment,
        indent: IndentLevel,
        output: &mut Output,
    ) {
        if comment.placement == CommentPlacement::OwnLine || output.line_comment_open {
            output.newline(indent);
        } else {
            output.append_space();
        }
        self.record_comment(comment);
        self.write_comment_text(comment, output);
    }

    fn write_comment_text(&self, comment: &Comment, output: &mut Output) {
        let mut lines = comment.normalized_lines();
        output.append(lines.next().expect("a comment has a first line"));
        for line in lines {
            output.newline_verbatim(line);
        }
        output.line_comment_open = comment.kind == CommentKind::Line;
    }

    fn record_inline_comments(&mut self, elements: &[Element]) {
        let (inline_comments, _) = self.split_comments(elements);
        self.record_comments(&inline_comments);
    }

    fn split_comments<'a>(
        &mut self,
        elements: &'a [Element],
    ) -> (Vec<&'a Comment>, Vec<&'a Comment>) {
        let mut comments = Vec::new();
        collect_comments(elements, &mut comments, &mut self.work);
        if comments.iter().any(|comment| comment.forces_line()) {
            return (Vec::new(), comments);
        }
        (comments, Vec::new())
    }

    fn boundary_comments<'a>(&mut self, elements: &'a [Element]) -> Vec<&'a Comment> {
        let mut comments = Vec::new();
        collect_comments(elements, &mut comments, &mut self.work);
        if comments.iter().any(|comment| comment.forces_line()) {
            return comments;
        }
        Vec::new()
    }

    fn inline_text(&mut self, elements: &[Element]) -> String {
        let include_comments = self.boundary_comments(elements).is_empty();
        let mut atoms = Vec::new();
        collect_atoms(elements, &mut atoms, include_comments, &mut self.work);
        format_atoms(&atoms)
    }

    fn record_comments(&mut self, comments: &[&Comment]) {
        for comment in comments {
            self.record_comment(comment);
        }
    }

    fn record_comment(&mut self, comment: &Comment) {
        let id = comment.id.0 as usize;
        assert!(!self.emitted_comments[id], "each CommentId is emitted once");
        self.emitted_comments[id] = true;
        self.emitted_count += 1;
    }
}

fn collect_comments<'a>(
    elements: &'a [Element],
    comments: &mut Vec<&'a Comment>,
    work: &mut WorkCounter,
) {
    for element in elements {
        work.add(1);
        match element {
            Element::Node(node) => collect_comments(&node.elements, comments, work),
            Element::Comment(comment) => comments.push(comment),
            Element::Token(_) => {}
        }
    }
}

fn collect_atoms<'a>(
    elements: &'a [Element],
    atoms: &mut Vec<Atom<'a>>,
    include_comments: bool,
    work: &mut WorkCounter,
) {
    for element in elements {
        work.add(1);
        match element {
            Element::Node(node) => collect_atoms(&node.elements, atoms, include_comments, work),
            Element::Token(token) => atoms.push(Atom::Token(token)),
            Element::Comment(comment) if include_comments && !comment.forces_line() => {
                atoms.push(Atom::Comment(comment));
            }
            Element::Comment(_) => {}
        }
    }
}
