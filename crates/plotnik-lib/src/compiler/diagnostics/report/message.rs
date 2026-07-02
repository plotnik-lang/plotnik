use rowan::TextRange;

use super::Span;

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
    // UnclosedString/UnclosedRegex rank first: an unterminated literal swallows
    // subsequent closing delimiters, so it is the root cause to show.
    UnclosedString,
    UnclosedRegex,
    UnclosedTree,
    UnclosedSequence,
    UnclosedAlternation,

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
    DuplicateCaptureInScope,
    IncompatibleCaptureTypes,
    IncompatibleStructShapes,
    TypeNameConflict,
    RedundantTypeAnnotation,

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
    UnsupportedSupertype,
    BareSupertype,
    ChildUnderLeafToken,
    NegatedRequiredField,
    UnsatisfiablePattern,
    QueryTooComplex,

    MissingDefName,

    // Placed last (lowest priority): `check`'s dry run reports these only when no
    // earlier-stage error exists.
    EmitFailed,
    BytecodeRejected,
    NoEntrypoints,
    EmptyQuery,
}

impl DiagnosticKind {
    /// Severity for this kind.
    pub fn severity(&self) -> Severity {
        match self {
            Self::UnusedBranchLabels
            | Self::RedundantTypeAnnotation
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
        let text = match self {
            Self::ExpectedSubtype => "e.g., `expression#binary_expression`",
            Self::ExpectedTypeName => "e.g., `::MyType`",
            Self::ExpectedFieldName => "e.g., `-value`",
            Self::EmptyTree => "use `(_)` to match any named node, or `_` for any node",
            Self::EmptyAnonymousNode => {
                "anonymous nodes match literal tokens, like `\"+\"` or `\";\"`"
            }
            Self::EmptySequence => "sequences must contain at least one expression",
            Self::EmptyAlternation => "alternations must contain at least one branch",
            Self::ErrorMissingOutsideParens => "write `(ERROR)` or `(MISSING \";\")`",
            Self::CaptureWithoutTarget => {
                "captures attach to the pattern before them: `(node) @name`"
            }
            Self::CaptureNameInvalid => "captures become fields in the output",
            Self::DefNameInvalid => "definitions become types in the output",
            Self::BranchLabelInvalid => "branch labels become variants of an enum in the output",
            Self::FieldNameInvalid => "fields come from the grammar and are snake_case",
            Self::TreeSitterSequenceSyntaxDeprecated => {
                "use `{(a) (b)}` to match a sequence of siblings"
            }
            Self::NegationSyntaxDeprecated => "use `-field` instead of `!field`",
            Self::SupertypeSlashDeprecated => {
                "use `supertype#subtype` instead of `supertype/subtype`"
            }
            Self::MixedAltBranches => "use all labels for an enum, or none for a merged struct",
            Self::DuplicateAlternationLabel => {
                "each branch label must be unique within an alternation"
            }
            Self::RecursionNoEscape => {
                "add a non-recursive branch to terminate: `[Base: ... Rec: (Self)]`"
            }
            Self::DirectRecursion => "recursive references must consume input before recursing",
            Self::AnchorWithoutContext => "wrap in a named node: `(parent . (child))`",
            Self::AnchorInAlternation => "use `[{(a) . (b)} (c)]` to anchor within a branch",
            Self::QuantifiedAnchor | Self::CapturedAnchor => {
                "anchors constrain position and produce no value"
            }
            Self::UnusedBranchLabels => {
                "capture the alternation (`[...] @name`) to make the labels enum variants, or remove them"
            }
            Self::UnclosedTree => "add `)` to close the node",
            Self::UnclosedSequence => "add `}` to close the sequence",
            Self::UnclosedAlternation => "add `]` to close the alternation",
            Self::UnclosedString => {
                "anonymous nodes match literal tokens; close the quote: `\"foo\"`"
            }
            Self::ExpectedExpression => {
                "an expression is a node `(kind)`, anonymous node `\"text\"`, sequence `{...}`, or alternation `[...]`"
            }
            Self::ExpectedPredicateValue => {
                "e.g., `(identifier == \"foo\")` or `(identifier =~ /foo/)`"
            }
            Self::FieldSequenceValue => {
                "a field holds a single child node; match one pattern, or move the sequence outside the field"
            }
            Self::UndefinedReference => {
                "`(Name)` uses a definition; define `Name = ...` or check the spelling"
            }
            Self::DuplicateCaptureInScope => {
                "rename one capture, or use an enum if they are mutually exclusive branches"
            }
            Self::PredicateOnNonLeaf => {
                "predicates match text content; apply them to a leaf node or an anonymous node like `\"foo\"`"
            }
            Self::EmptyRegex => {
                "put a pattern between the slashes, e.g. `=~ /^foo/`, or use a string predicate like `== \"foo\"`"
            }
            Self::RegexBackreference => {
                "the regex engine is linear-time and cannot match backreferences; rewrite without `\\1`"
            }
            Self::RegexLookaround => {
                "the regex engine cannot match lookaround; match the surrounding context with the query pattern instead"
            }
            Self::RegexNamedCapture => {
                "regex captures are inert in plotnik; capture nodes with `@name` outside the regex"
            }
            Self::InvalidSupertypeSyntax => {
                "supertypes refine node kinds, not references: write `(supertype#subtype)` or just `(RefName)`"
            }
            Self::ErrorTakesNoArguments => {
                "`(ERROR)` matches any error node as a leaf; use `(MISSING \"x\")` to match a missing token"
            }
            Self::RefCannotHaveChildren => {
                "a reference reuses a definition as a whole: write `(Expr)`, or define a node kind to add children"
            }
            Self::NoEntrypoints => {
                "every definition must produce a value; `.`, `-field`, and `.!` constrain position but produce nothing"
            }
            Self::EmptyQuery => "add a definition, e.g. `Q = (identifier) @id`",
            Self::UnsupportedSupertype | Self::BareSupertype => {
                "match the concrete subtypes with an alternation, e.g. `[(a) (b)]`"
            }
            _ => return None,
        };

        Some(text)
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
            Self::MixedAltBranches => "cannot mix enum and union branches",
            Self::DuplicateAlternationLabel => "duplicate branch label",
            Self::RecursionNoEscape => "infinite recursion: no escape path",
            Self::DirectRecursion => "infinite recursion: cycle consumes no input",
            Self::FieldSequenceValue => "field cannot match a sequence",
            Self::AnchorWithoutContext => "anchor needs an enclosing node",
            Self::IncompatibleTypes => "incompatible types",
            Self::UnusedBranchLabels => "branch labels have no effect without capture",
            Self::StrictDimensionalityViolation => {
                "a repeated capture must be collected into a list"
            }
            Self::MultiElementScalarCapture => "a captured pattern must match exactly one node",
            Self::DuplicateCaptureInScope => "duplicate capture in scope",
            Self::IncompatibleCaptureTypes => "incompatible capture types",
            Self::IncompatibleStructShapes => "incompatible struct shapes",
            Self::TypeNameConflict => "conflicting type name",
            Self::RedundantTypeAnnotation => "redundant type annotation",
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
            Self::UnsupportedSupertype => "matching a supertype is not supported yet",
            Self::BareSupertype => "supertype is not a matchable node kind",
            Self::ChildUnderLeafToken => "leaf tokens have no child nodes",
            Self::NegatedRequiredField => "this field is always present",
            Self::UnsatisfiablePattern => "pattern can never match",
            Self::QueryTooComplex => "query too complex to compile",
            Self::MissingDefName => "definition must be named",
            Self::EmitFailed => "bytecode emission failed",
            Self::BytecodeRejected => "query compiles to invalid bytecode",
            Self::NoEntrypoints => "query produces no entrypoints",
            Self::EmptyQuery => "query defines nothing",
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
            // The detail leads with the specific conflict; the kind name is not prefixed.
            Self::IncompatibleTypes => "{}".to_string(),
            Self::StrictDimensionalityViolation => "{}".to_string(),
            Self::MultiElementScalarCapture => "{}".to_string(),
            Self::TypeNameConflict => "type name `{}` is already used for a different type".to_string(),
            Self::RedundantTypeAnnotation => "this type annotation {}".to_string(),
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
            Self::UnsupportedSupertype => {
                "matching the `{}#` supertype is not supported yet".to_string()
            }
            Self::BareSupertype => "`{}` is a supertype, not a node kind".to_string(),
            Self::ChildUnderLeafToken => "`{}` is a leaf token — it has no child nodes".to_string(),
            Self::NegatedRequiredField => "`-{}` can never match".to_string(),
            // The detail, when present, is the crafted message; bare emits use the summary.
            Self::UnsatisfiablePattern => "{}".to_string(),
            Self::MixedAltBranches => "cannot mix enum and union branches: {}".to_string(),
            Self::DuplicateAlternationLabel => {
                "branch label `{}` is already used in this alternation".to_string()
            }
            // The detail (an `EmitError`/`ModuleError` Display) is already a complete message.
            Self::EmitFailed | Self::BytecodeRejected => "{}".to_string(),
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
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Diagnostic {
    pub(crate) kind: DiagnosticKind,
    /// The range shown to the user (underlined in output).
    pub(crate) span: Span,
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
    pub(crate) fn new(kind: DiagnosticKind, span: Span) -> Self {
        Self {
            kind,
            span,
            suppression_range: span.range,
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
            u32::from(self.span.range.start()),
            u32::from(self.span.range.end()),
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
