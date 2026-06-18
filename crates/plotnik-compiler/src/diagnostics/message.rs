use rowan::TextRange;

use super::{SourceId, Span};

/// Diagnostic kinds ordered by priority (highest priority first).
///
/// When two diagnostics have overlapping spans, the higher-priority one
/// suppresses the lower-priority one. This prevents cascading error noise.
///
/// Priority rationale:
/// - Unclosed delimiters cause massive cascading errors downstream
/// - Expected token errors are root causes the user should fix first
/// - Invalid syntax usage is a specific mistake at a location
/// - Naming validation errors are convention violations
/// - Semantic errors assume valid syntax
/// - Structural observations are often consequences of earlier errors
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticKind {
    // UnclosedString ranks first: an unterminated string swallows subsequent closing delimiters.
    UnclosedString,
    UnclosedTree,
    UnclosedSequence,
    UnclosedAlternation,
    UnclosedRegex,

    ExpectedExpression,
    ExpectedTypeName,
    ExpectedFieldName,
    ExpectedSubtype,
    ExpectedPredicateValue,

    EmptyTree,
    EmptyAnonymousNode,
    EmptySequence,
    EmptyAlternation,
    BareIdentifier,
    InvalidSeparator,
    AnchorInAlternation,
    QuantifiedAnchor,
    CapturedAnchor,
    InvalidFieldEquals,
    InvalidSupertypeSyntax,
    InvalidTypeAnnotationSyntax,
    ErrorTakesNoArguments,
    RefCannotHaveChildren,
    ErrorMissingOutsideParens,
    UnsupportedPredicate,
    UnexpectedToken,
    CaptureWithoutTarget,

    CaptureNameInvalid,
    DefNameInvalid,
    BranchLabelInvalid,
    FieldNameInvalid,
    TypeNameInvalid,
    TreeSitterSequenceSyntaxDeprecated,
    NegationSyntaxDeprecated,
    SupertypeSlashDeprecated,

    DuplicateDefinition,
    UndefinedReference,
    MixedAltBranches,
    DuplicateAlternationLabel,
    RecursionNoEscape,
    DirectRecursion,
    FieldSequenceValue,
    AnchorWithoutContext,

    IncompatibleTypes,
    UnusedBranchLabels,
    StrictDimensionalityViolation,
    MultiElementScalarCapture,
    UncapturedOutputWithCaptures,
    AmbiguousUncapturedOutputs,
    DuplicateCaptureInScope,
    IncompatibleCaptureTypes,
    IncompatibleStructShapes,

    PredicateOnNonLeaf,
    EmptyRegex,
    RegexBackreference,
    RegexLookaround,
    RegexNamedCapture,
    RegexSyntaxError,

    UnknownNodeKind,
    UnknownField,
    FieldNotOnNodeKind,
    InvalidFieldChildType,
    InvalidChildType,
    InvalidSubtype,
    ChildUnderLeafToken,
    NegatedRequiredField,

    MissingDefName,
}

impl DiagnosticKind {
    /// Severity for this kind.
    pub fn severity(&self) -> Severity {
        match self {
            Self::UnusedBranchLabels
            | Self::TreeSitterSequenceSyntaxDeprecated
            | Self::NegationSyntaxDeprecated
            | Self::SupertypeSlashDeprecated => Severity::Warning,
            _ => Severity::Error,
        }
    }

    /// Whether this kind suppresses `other` when spans overlap.
    ///
    /// Uses enum discriminant ordering: lower position = higher priority.
    /// A higher-priority diagnostic suppresses lower-priority ones in the same span.
    pub fn suppresses(&self, other: &DiagnosticKind) -> bool {
        self < other
    }

    /// Structural errors are Unclosed* - they cause cascading errors but
    /// should be suppressed by root-cause errors at the same position.
    pub fn is_structural_error(&self) -> bool {
        matches!(
            self,
            Self::UnclosedTree
                | Self::UnclosedSequence
                | Self::UnclosedAlternation
                | Self::UnclosedRegex
                | Self::UnclosedString
        )
    }

    /// Root cause errors - user omitted something required.
    /// These suppress structural errors at the same position.
    pub fn is_root_cause_error(&self) -> bool {
        matches!(
            self,
            Self::ExpectedExpression
                | Self::ExpectedTypeName
                | Self::ExpectedFieldName
                | Self::ExpectedSubtype
                | Self::ExpectedPredicateValue
        )
    }

    /// Consequence errors - often caused by earlier parse errors.
    /// These get suppressed when any root-cause or structural error exists.
    pub fn is_cascade_consequence(&self) -> bool {
        matches!(self, Self::MissingDefName)
    }

    /// Default hint for this kind, automatically included in diagnostics.
    /// Call sites can add additional hints for context-specific information.
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Self::ExpectedSubtype => Some("e.g., `expression#binary_expression`"),
            Self::ExpectedTypeName => Some("e.g., `::MyType`"),
            Self::ExpectedFieldName => Some("e.g., `-value`"),
            Self::EmptyTree => Some("use `(_)` to match any named node, or `_` for any node"),
            Self::EmptyAnonymousNode => {
                Some("anonymous nodes match literal tokens, like `\"+\"` or `\";\"`")
            }
            Self::EmptySequence => Some("sequences must contain at least one expression"),
            Self::EmptyAlternation => Some("alternations must contain at least one branch"),
            Self::ErrorMissingOutsideParens => Some("write `(ERROR)` or `(MISSING \";\")`"),
            Self::CaptureWithoutTarget => {
                Some("captures attach to the pattern before them: `(node) @name`")
            }
            Self::CaptureNameInvalid => Some("captures become fields in the output"),
            Self::DefNameInvalid => Some("definitions become types in the output"),
            Self::BranchLabelInvalid => {
                Some("branch labels become variants of an enum in the output")
            }
            Self::FieldNameInvalid => Some("fields come from the grammar and are snake_case"),
            Self::TreeSitterSequenceSyntaxDeprecated => {
                Some("use `{(a) (b)}` to match a sequence of siblings")
            }
            Self::NegationSyntaxDeprecated => Some("use `-field` instead of `!field`"),
            Self::SupertypeSlashDeprecated => {
                Some("use `supertype#subtype` instead of `supertype/subtype`")
            }
            Self::MixedAltBranches => {
                Some("use all labels for an enum, or none for a merged struct")
            }
            Self::DuplicateAlternationLabel => {
                Some("each branch label must be unique within an alternation")
            }
            Self::RecursionNoEscape => {
                Some("add a non-recursive branch to terminate: `[Base: ... Rec: (Self)]`")
            }
            Self::DirectRecursion => {
                Some("recursive references must consume input before recursing")
            }
            Self::AnchorWithoutContext => Some("wrap in a named node: `(parent . (child))`"),
            Self::AnchorInAlternation => Some("use `[{(a) . (b)} (c)]` to anchor within a branch"),
            Self::QuantifiedAnchor | Self::CapturedAnchor => {
                Some("anchors constrain position and produce no value")
            }
            Self::UncapturedOutputWithCaptures => Some("add `@name` to capture the output"),
            Self::AmbiguousUncapturedOutputs => {
                Some("capture each expression explicitly: `(X) @x (Y) @y`")
            }
            Self::MultiElementScalarCapture => {
                Some("add internal captures: `{(a) @a (b) @b}* @items`")
            }
            Self::UnclosedTree => Some("add `)` to close the node"),
            Self::UnclosedSequence => Some("add `}` to close the sequence"),
            Self::UnclosedAlternation => Some("add `]` to close the alternation"),
            Self::UnclosedString => {
                Some("anonymous nodes match literal tokens; close the quote: `\"foo\"`")
            }
            Self::ExpectedExpression => Some(
                "an expression is a node `(kind)`, anonymous node `\"text\"`, sequence `{...}`, or alternation `[...]`",
            ),
            Self::ExpectedPredicateValue => {
                Some("e.g., `(identifier == \"foo\")` or `(identifier =~ /foo/)`")
            }
            Self::FieldSequenceValue => Some(
                "a field holds a single child node; match one pattern, or move the sequence outside the field",
            ),
            Self::UndefinedReference => {
                Some("`(Name)` uses a definition; define `Name = ...` or check the spelling")
            }
            Self::DuplicateCaptureInScope => Some(
                "rename one capture, or use a labeled alternation if they are mutually exclusive branches",
            ),
            Self::PredicateOnNonLeaf => Some(
                "predicates match text content; apply them to a leaf node or an anonymous node like `\"foo\"`",
            ),
            Self::EmptyRegex => Some(
                "put a pattern between the slashes, e.g. `=~ /^foo/`, or use a string predicate like `== \"foo\"`",
            ),
            Self::RegexBackreference => Some(
                "the regex engine is linear-time and cannot match backreferences; rewrite without `\\1`",
            ),
            Self::RegexLookaround => Some(
                "the regex engine cannot match lookaround; match the surrounding context with the query pattern instead",
            ),
            Self::RegexNamedCapture => Some(
                "regex captures are inert in plotnik; capture nodes with `@name` outside the regex",
            ),
            Self::InvalidSupertypeSyntax => Some(
                "supertypes refine node kinds, not references: write `(supertype#subtype)` or just `(RefName)`",
            ),
            Self::ErrorTakesNoArguments => Some(
                "`(ERROR)` matches any error node as a leaf; use `(MISSING \"x\")` to match a missing token",
            ),
            Self::RefCannotHaveChildren => Some(
                "a reference reuses a definition as a whole: write `(Expr)`, or define a node kind to add children",
            ),
            _ => None,
        }
    }

    pub fn summary(&self) -> &'static str {
        match self {
            Self::UnclosedTree => "missing closing `)`",
            Self::UnclosedSequence => "missing closing `}`",
            Self::UnclosedAlternation => "missing closing `]`",
            Self::UnclosedRegex => "missing closing `/` for regex",
            Self::UnclosedString => "unterminated string",
            Self::ExpectedExpression => "expected an expression",
            Self::ExpectedTypeName => "expected a type name after `::`",
            Self::ExpectedFieldName => "expected a field name",
            Self::ExpectedSubtype => "expected a subtype after `/`",
            Self::ExpectedPredicateValue => "expected a string or regex after the operator",
            Self::EmptyTree => "empty `()` matches nothing",
            Self::EmptyAnonymousNode => "empty string matches nothing",
            Self::EmptySequence => "empty `{}` matches nothing",
            Self::EmptyAlternation => "empty `[]` matches nothing",
            Self::BareIdentifier => "node kinds must be parenthesized",
            Self::InvalidSeparator => "patterns are separated by whitespace",
            Self::AnchorInAlternation => "anchors cannot appear directly in alternations",
            Self::QuantifiedAnchor => "anchors cannot be quantified",
            Self::CapturedAnchor => "anchors cannot be captured",
            Self::InvalidFieldEquals => "fields use `:`, not `=`",
            Self::InvalidSupertypeSyntax => "references cannot have supertypes",
            Self::InvalidTypeAnnotationSyntax => "type annotations use `::`, not `:`",
            Self::ErrorTakesNoArguments => "`(ERROR)` cannot have children",
            Self::RefCannotHaveChildren => "references cannot have children",
            Self::ErrorMissingOutsideParens => "`ERROR` and `MISSING` must be parenthesized",
            Self::UnsupportedPredicate => "tree-sitter predicates are not supported",
            Self::UnexpectedToken => "unexpected token",
            Self::CaptureWithoutTarget => "expected a capture name after `@`",
            Self::CaptureNameInvalid => "capture names must be snake_case",
            Self::DefNameInvalid => "definition names must be PascalCase",
            Self::BranchLabelInvalid => "branch labels must be PascalCase",
            Self::FieldNameInvalid => "field names must be snake_case",
            Self::TypeNameInvalid => "type names must be PascalCase",
            Self::TreeSitterSequenceSyntaxDeprecated => {
                "parenthesized sequences are tree-sitter syntax"
            }
            Self::NegationSyntaxDeprecated => "`!field` negation is deprecated",
            Self::SupertypeSlashDeprecated => "`supertype/subtype` paths are tree-sitter syntax",
            Self::DuplicateDefinition => "duplicate definition",
            Self::UndefinedReference => "undefined reference",
            Self::MixedAltBranches => "cannot mix labeled and unlabeled branches",
            Self::DuplicateAlternationLabel => "duplicate branch label",
            Self::RecursionNoEscape => "infinite recursion: no escape path",
            Self::DirectRecursion => "infinite recursion: cycle consumes no input",
            Self::FieldSequenceValue => "field cannot match a sequence",
            Self::AnchorWithoutContext => "boundary anchor requires parent node context",
            Self::IncompatibleTypes => "incompatible types",
            Self::UnusedBranchLabels => "branch labels have no effect without capture",
            Self::StrictDimensionalityViolation => {
                "quantifier with captures requires a struct capture"
            }
            Self::MultiElementScalarCapture => {
                "cannot capture multi-element pattern as scalar array"
            }
            Self::UncapturedOutputWithCaptures => {
                "output-producing expression requires capture when siblings have captures"
            }
            Self::AmbiguousUncapturedOutputs => {
                "multiple expressions produce output without capture"
            }
            Self::DuplicateCaptureInScope => "duplicate capture in scope",
            Self::IncompatibleCaptureTypes => "incompatible capture types",
            Self::IncompatibleStructShapes => "incompatible struct shapes",
            Self::PredicateOnNonLeaf => {
                "predicates match text content, but this node can contain children"
            }
            Self::EmptyRegex => "empty regex pattern",
            Self::RegexBackreference => "backreferences are not supported in regex",
            Self::RegexLookaround => "lookahead/lookbehind is not supported in regex",
            Self::RegexNamedCapture => "named captures are not supported in regex",
            Self::RegexSyntaxError => "invalid regex syntax",
            Self::UnknownNodeKind => "unknown node kind",
            Self::UnknownField => "unknown field",
            Self::FieldNotOnNodeKind => "field not valid on this node kind",
            Self::InvalidFieldChildType => "node kind not valid for this field",
            Self::InvalidChildType => "node kind not valid as child",
            Self::InvalidSubtype => "node kind is not a subtype of this kind",
            Self::ChildUnderLeafToken => "leaf tokens have no child nodes",
            Self::NegatedRequiredField => "this field is always present",
            Self::MissingDefName => "definition must be named",
        }
    }

    /// Template for custom messages; `{}` is replaced by the caller-provided detail.
    pub fn template(&self) -> String {
        match self {
            // The detail IS the full message.
            Self::UnexpectedToken | Self::BareIdentifier => "{}".to_string(),

            Self::RefCannotHaveChildren => {
                "`{}` is a reference and cannot have children".to_string()
            }
            Self::FieldSequenceValue => "field `{}` cannot match a sequence".to_string(),
            Self::DuplicateDefinition => "`{}` is already defined".to_string(),
            Self::UndefinedReference => "`{}` is not defined".to_string(),
            Self::IncompatibleTypes => "incompatible types: {}".to_string(),
            Self::StrictDimensionalityViolation => "{}".to_string(),
            Self::MultiElementScalarCapture => "{}".to_string(),
            Self::AmbiguousUncapturedOutputs => "{}".to_string(),
            Self::DuplicateCaptureInScope => {
                "capture `@{}` already defined in this scope".to_string()
            }
            Self::IncompatibleCaptureTypes => {
                "capture `@{}` has incompatible types across branches".to_string()
            }
            Self::IncompatibleStructShapes => {
                "capture `@{}` has incompatible struct fields across branches".to_string()
            }
            Self::UnknownNodeKind => "`{}` is not a valid node kind".to_string(),
            Self::UnknownField => "`{}` is not a valid field".to_string(),
            Self::FieldNotOnNodeKind => "field `{}` is not valid on this node kind".to_string(),
            Self::InvalidFieldChildType => "{}".to_string(),
            Self::InvalidChildType => "`{}` cannot be a child of this node".to_string(),
            Self::InvalidSubtype => "{}".to_string(),
            Self::ChildUnderLeafToken => "`{}` is a leaf token — it has no child nodes".to_string(),
            Self::NegatedRequiredField => "`-{}` can never match".to_string(),
            Self::MixedAltBranches => "cannot mix labeled and unlabeled branches: {}".to_string(),
            Self::DuplicateAlternationLabel => {
                "branch label `{}` is already used in this alternation".to_string()
            }
            _ => format!("{}: {{}}", self.summary()),
        }
    }

    /// Render the final message.
    ///
    /// - `None` → returns `summary()`
    /// - `Some(detail)` → returns `template()` with `{}` replaced by detail
    pub fn render(&self, msg: Option<&str>) -> String {
        match msg {
            None => self.summary().to_string(),
            Some(detail) => self.template().replace("{}", detail),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Error,
    Warning,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub(crate) description: String,
    pub(crate) replacement: String,
}

impl Fix {
    pub fn new(description: impl Into<String>, replacement: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            replacement: replacement.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Related {
    pub(crate) span: Span,
    pub(crate) message: String,
}

impl Related {
    pub fn new(source: SourceId, range: TextRange, message: impl Into<String>) -> Self {
        Self {
            span: Span::new(source, range),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Diagnostic {
    pub(crate) kind: DiagnosticKind,
    /// Which source file this diagnostic belongs to.
    pub(crate) source: SourceId,
    /// The range shown to the user (underlined in output).
    pub(crate) range: TextRange,
    /// The range used for suppression logic. Errors within another error's
    /// suppression_range may be suppressed. Defaults to `range` but can be
    /// set to a parent context (e.g., enclosing tree span) for better cascading
    /// error suppression.
    pub(crate) suppression_range: TextRange,
    pub(crate) message: String,
    pub(crate) fix: Option<Fix>,
    pub(crate) related: Vec<Related>,
    pub(crate) hints: Vec<String>,
}

impl Diagnostic {
    /// New message with the kind's fallback text; `DiagnosticBuilder::detail` overrides it.
    pub(crate) fn new(source: SourceId, kind: DiagnosticKind, range: TextRange) -> Self {
        Self {
            kind,
            source,
            range,
            suppression_range: range,
            message: kind.summary().to_string(),
            fix: None,
            related: Vec::new(),
            hints: Vec::new(),
        }
    }

    pub(crate) fn severity(&self) -> Severity {
        self.kind.severity()
    }

    pub(crate) fn is_error(&self) -> bool {
        self.severity() == Severity::Error
    }

    pub(crate) fn is_warning(&self) -> bool {
        self.severity() == Severity::Warning
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} at {}..{}: {}",
            self.severity(),
            u32::from(self.range.start()),
            u32::from(self.range.end()),
            self.message
        )?;
        if let Some(fix) = &self.fix {
            write!(f, " (fix: {})", fix.description)?;
        }
        for related in &self.related {
            write!(
                f,
                " (related: {} at {}..{})",
                related.message,
                u32::from(related.span.range.start()),
                u32::from(related.span.range.end())
            )?;
        }
        for hint in &self.hints {
            write!(f, " (hint: {})", hint)?;
        }
        Ok(())
    }
}
