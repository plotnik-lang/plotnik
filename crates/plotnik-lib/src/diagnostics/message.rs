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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiagnosticKind {
    // These cause cascading errors throughout the rest of the file
    UnclosedTree,
    UnclosedSequence,
    UnclosedAlternation,

    // User omitted something required - root cause errors
    ExpectedExpression,
    ExpectedTypeName,
    ExpectedCaptureName,
    ExpectedFieldName,
    ExpectedSubtype,

    // User wrote something that doesn't belong
    EmptyTree,
    BareIdentifier,
    InvalidSeparator,
    InvalidFieldEquals,
    InvalidSupertypeSyntax,
    InvalidTypeAnnotationSyntax,
    ErrorTakesNoArguments,
    RefCannotHaveChildren,
    ErrorMissingOutsideParens,
    UnsupportedPredicate,
    UnexpectedToken,
    CaptureWithoutTarget,
    LowercaseBranchLabel,

    // Convention violations - fixable with suggestions
    CaptureNameHasDots,
    CaptureNameHasHyphens,
    CaptureNameUppercase,
    DefNameLowercase,
    DefNameHasSeparators,
    BranchLabelHasSeparators,
    FieldNameHasDots,
    FieldNameHasHyphens,
    FieldNameUppercase,
    TypeNameInvalidChars,
    TreeSitterSequenceSyntax,

    // Valid syntax, invalid semantics
    DuplicateDefinition,
    UndefinedReference,
    MixedAltBranches,
    RecursionNoEscape,
    DirectRecursion,
    FieldSequenceValue,

    // Type inference errors
    IncompatibleTypes,
    MultiCaptureQuantifierNoName,
    UnusedBranchLabels,
    StrictDimensionalityViolation,
    DuplicateCaptureInScope,
    IncompatibleCaptureTypes,
    IncompatibleStructShapes,

    // Link pass - grammar validation
    UnknownNodeType,
    UnknownField,
    FieldNotOnNodeType,
    InvalidFieldChildType,
    InvalidChildType,

    // Often consequences of earlier errors
    UnnamedDef,
}

impl DiagnosticKind {
    /// Default severity for this kind. Can be overridden by policy.
    pub fn default_severity(&self) -> Severity {
        match self {
            Self::UnusedBranchLabels | Self::TreeSitterSequenceSyntax => Severity::Warning,
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
            Self::UnclosedTree | Self::UnclosedSequence | Self::UnclosedAlternation
        )
    }

    /// Root cause errors - user omitted something required.
    /// These suppress structural errors at the same position.
    pub fn is_root_cause_error(&self) -> bool {
        matches!(
            self,
            Self::ExpectedExpression
                | Self::ExpectedTypeName
                | Self::ExpectedCaptureName
                | Self::ExpectedFieldName
                | Self::ExpectedSubtype
        )
    }

    /// Consequence errors - often caused by earlier parse errors.
    /// These get suppressed when any root-cause or structural error exists.
    pub fn is_consequence_error(&self) -> bool {
        matches!(self, Self::UnnamedDef)
    }

    /// Default hint for this kind, automatically included in diagnostics.
    /// Call sites can add additional hints for context-specific information.
    pub fn default_hint(&self) -> Option<&'static str> {
        match self {
            Self::ExpectedSubtype => Some("e.g., `expression/binary_expression`"),
            Self::ExpectedTypeName => Some("e.g., `::MyType` or `::string`"),
            Self::ExpectedFieldName => Some("e.g., `!value`"),
            Self::EmptyTree => Some("use `(_)` to match any named node, or `_` for any node"),
            Self::TreeSitterSequenceSyntax => Some("use `{...}` for sequences"),
            Self::MixedAltBranches => {
                Some("use all labels for a tagged union, or none for a merged struct")
            }
            Self::RecursionNoEscape => {
                Some("add a non-recursive branch to terminate: `[Base: ... Rec: (Self)]`")
            }
            Self::DirectRecursion => {
                Some("recursive references must consume input before recursing")
            }
            _ => None,
        }
    }

    /// Base message for this diagnostic kind, used when no custom message is provided.
    pub fn fallback_message(&self) -> &'static str {
        match self {
            // Unclosed delimiters
            Self::UnclosedTree => "missing closing `)`",
            Self::UnclosedSequence => "missing closing `}`",
            Self::UnclosedAlternation => "missing closing `]`",

            // Expected token errors
            Self::ExpectedExpression => "expected an expression",
            Self::ExpectedTypeName => "expected type name",
            Self::ExpectedCaptureName => "expected capture name",
            Self::ExpectedFieldName => "expected field name",
            Self::ExpectedSubtype => "expected subtype name",

            // Invalid syntax
            Self::EmptyTree => "empty `()` is not allowed",
            Self::BareIdentifier => "bare identifier is not valid",
            Self::InvalidSeparator => "unexpected separator",
            Self::InvalidFieldEquals => "use `:` instead of `=`",
            Self::InvalidSupertypeSyntax => "references cannot have supertypes",
            Self::InvalidTypeAnnotationSyntax => "use `::` for type annotations",
            Self::ErrorTakesNoArguments => "`(ERROR)` cannot have children",
            Self::RefCannotHaveChildren => "references cannot have children",
            Self::ErrorMissingOutsideParens => "special node requires parentheses",
            Self::UnsupportedPredicate => "predicates are not supported",
            Self::UnexpectedToken => "unexpected token",
            Self::CaptureWithoutTarget => "capture has no target",
            Self::LowercaseBranchLabel => "branch label must start with uppercase",

            // Naming convention violations
            Self::CaptureNameHasDots => "capture names cannot contain `.`",
            Self::CaptureNameHasHyphens => "capture names cannot contain `-`",
            Self::CaptureNameUppercase => "capture names must be lowercase",
            Self::DefNameLowercase => "definition names must start uppercase",
            Self::DefNameHasSeparators => "definition names must be PascalCase",
            Self::BranchLabelHasSeparators => "branch labels must be PascalCase",
            Self::FieldNameHasDots => "field names cannot contain `.`",
            Self::FieldNameHasHyphens => "field names cannot contain `-`",
            Self::FieldNameUppercase => "field names must be lowercase",
            Self::TypeNameInvalidChars => "type names cannot contain `.` or `-`",
            Self::TreeSitterSequenceSyntax => "tree-sitter sequence syntax",

            // Semantic errors
            Self::DuplicateDefinition => "duplicate definition",
            Self::UndefinedReference => "undefined reference",
            Self::MixedAltBranches => "cannot mix labeled and unlabeled branches",
            Self::RecursionNoEscape => "infinite recursion: no escape path",
            Self::DirectRecursion => "infinite recursion: cycle consumes no input",
            Self::FieldSequenceValue => "field cannot match a sequence",

            // Type inference
            Self::IncompatibleTypes => "incompatible types",
            Self::MultiCaptureQuantifierNoName => {
                "quantified expression with multiple captures requires a struct capture"
            }
            Self::UnusedBranchLabels => "branch labels have no effect without capture",
            Self::StrictDimensionalityViolation => {
                "quantifier with captures requires a struct capture"
            }
            Self::DuplicateCaptureInScope => "duplicate capture in scope",
            Self::IncompatibleCaptureTypes => "incompatible capture types",
            Self::IncompatibleStructShapes => "incompatible struct shapes",

            // Link pass - grammar validation
            Self::UnknownNodeType => "unknown node type",
            Self::UnknownField => "unknown field",
            Self::FieldNotOnNodeType => "field not valid on this node type",
            Self::InvalidFieldChildType => "node type not valid for this field",
            Self::InvalidChildType => "node type not valid as child",

            // Structural
            Self::UnnamedDef => "definition must be named",
        }
    }

    /// Template for custom messages. Contains `{}` placeholder for caller-provided detail.
    pub fn custom_message(&self) -> String {
        match self {
            // Special formatting for references
            Self::RefCannotHaveChildren => {
                "`{}` is a reference and cannot have children".to_string()
            }
            Self::FieldSequenceValue => "field `{}` cannot match a sequence".to_string(),

            // Semantic errors with name context
            Self::DuplicateDefinition => "`{}` is already defined".to_string(),
            Self::UndefinedReference => "`{}` is not defined".to_string(),
            Self::IncompatibleTypes => "incompatible types: {}".to_string(),

            // Type inference errors with context
            Self::StrictDimensionalityViolation => "{}".to_string(),
            Self::DuplicateCaptureInScope => {
                "capture `@{}` already defined in this scope".to_string()
            }
            Self::IncompatibleCaptureTypes => {
                "capture `@{}` has incompatible types across branches".to_string()
            }
            Self::IncompatibleStructShapes => {
                "capture `@{}` has incompatible struct fields across branches".to_string()
            }

            // Link pass errors with context
            Self::UnknownNodeType => "`{}` is not a valid node type".to_string(),
            Self::UnknownField => "`{}` is not a valid field".to_string(),
            Self::FieldNotOnNodeType => "field `{}` is not valid on this node type".to_string(),
            Self::InvalidFieldChildType => "node type `{}` is not valid for this field".to_string(),
            Self::InvalidChildType => "`{}` cannot be a child of this node".to_string(),

            // Alternation mixing
            Self::MixedAltBranches => "cannot mix labeled and unlabeled branches: {}".to_string(),

            // Unclosed with context
            Self::UnclosedTree | Self::UnclosedSequence | Self::UnclosedAlternation => {
                format!("{}; {{}}", self.fallback_message())
            }

            // Type annotation specifics
            Self::InvalidTypeAnnotationSyntax => "use `::` for type annotations: {}".to_string(),

            // Named def (no custom message needed; suggestion goes in hint)
            Self::UnnamedDef => self.fallback_message().to_string(),

            // Standard pattern: fallback + context
            _ => format!("{}: {{}}", self.fallback_message()),
        }
    }

    /// Render the final message.
    ///
    /// - `None` → returns `fallback_message()`
    /// - `Some(detail)` → returns `custom_message()` with `{}` replaced by detail
    pub fn message(&self, msg: Option<&str>) -> String {
        match msg {
            None => self.fallback_message().to_string(),
            Some(detail) => self.custom_message().replace("{}", detail),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
    pub(crate) replacement: String,
    pub(crate) description: String,
}

impl Fix {
    pub fn new(replacement: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            replacement: replacement.into(),
            description: description.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedInfo {
    pub(crate) span: Span,
    pub(crate) message: String,
}

impl RelatedInfo {
    pub fn new(source: SourceId, range: TextRange, message: impl Into<String>) -> Self {
        Self {
            span: Span::new(source, range),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiagnosticMessage {
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
    pub(crate) related: Vec<RelatedInfo>,
    pub(crate) hints: Vec<String>,
}

impl DiagnosticMessage {
    pub(crate) fn new(
        source: SourceId,
        kind: DiagnosticKind,
        range: TextRange,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            source,
            range,
            suppression_range: range,
            message: message.into(),
            fix: None,
            related: Vec::new(),
            hints: Vec::new(),
        }
    }

    pub(crate) fn with_default_message(
        source: SourceId,
        kind: DiagnosticKind,
        range: TextRange,
    ) -> Self {
        Self::new(source, kind, range, kind.fallback_message())
    }

    pub(crate) fn severity(&self) -> Severity {
        self.kind.default_severity()
    }

    pub(crate) fn is_error(&self) -> bool {
        self.severity() == Severity::Error
    }

    pub(crate) fn is_warning(&self) -> bool {
        self.severity() == Severity::Warning
    }
}

impl std::fmt::Display for DiagnosticMessage {
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
