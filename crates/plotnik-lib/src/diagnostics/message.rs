use rowan::TextRange;

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

    // Valid syntax, invalid semantics
    DuplicateDefinition,
    UndefinedReference,
    MixedAltBranches,
    RecursionNoEscape,
    FieldSequenceValue,

    // Often consequences of earlier errors
    UnnamedDefNotLast,
}

impl DiagnosticKind {
    /// Default severity for this kind. Can be overridden by policy.
    pub fn default_severity(&self) -> Severity {
        Severity::Error
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
        matches!(self, Self::UnnamedDefNotLast)
    }

    /// Base message for this diagnostic kind, used when no custom message is provided.
    pub fn fallback_message(&self) -> &'static str {
        match self {
            // Unclosed delimiters
            Self::UnclosedTree => "unclosed tree",
            Self::UnclosedSequence => "unclosed sequence",
            Self::UnclosedAlternation => "unclosed alternation",

            // Expected token errors
            Self::ExpectedExpression => "expected expression",
            Self::ExpectedTypeName => "expected type name",
            Self::ExpectedCaptureName => "expected capture name",
            Self::ExpectedFieldName => "expected field name",
            Self::ExpectedSubtype => "expected subtype",

            // Invalid token/syntax usage
            Self::EmptyTree => "empty tree expression",
            Self::BareIdentifier => "bare identifier not allowed",
            Self::InvalidSeparator => "invalid separator",
            Self::InvalidFieldEquals => "invalid field syntax",
            Self::InvalidSupertypeSyntax => "invalid supertype syntax",
            Self::InvalidTypeAnnotationSyntax => "invalid type annotation syntax",
            Self::ErrorTakesNoArguments => "(ERROR) takes no arguments",
            Self::RefCannotHaveChildren => "reference cannot contain children",
            Self::ErrorMissingOutsideParens => "ERROR/MISSING outside parentheses",
            Self::UnsupportedPredicate => "unsupported predicate",
            Self::UnexpectedToken => "unexpected token",
            Self::CaptureWithoutTarget => "capture without target",
            Self::LowercaseBranchLabel => "lowercase branch label",

            // Naming validation
            Self::CaptureNameHasDots => "capture name contains dots",
            Self::CaptureNameHasHyphens => "capture name contains hyphens",
            Self::CaptureNameUppercase => "capture name starts with uppercase",
            Self::DefNameLowercase => "definition name starts with lowercase",
            Self::DefNameHasSeparators => "definition name contains separators",
            Self::BranchLabelHasSeparators => "branch label contains separators",
            Self::FieldNameHasDots => "field name contains dots",
            Self::FieldNameHasHyphens => "field name contains hyphens",
            Self::FieldNameUppercase => "field name starts with uppercase",
            Self::TypeNameInvalidChars => "type name contains invalid characters",

            // Semantic errors
            Self::DuplicateDefinition => "duplicate definition",
            Self::UndefinedReference => "undefined reference",
            Self::MixedAltBranches => "mixed tagged and untagged branches in alternation",
            Self::RecursionNoEscape => "recursive pattern can never match",
            Self::FieldSequenceValue => "field value must be a single node",

            // Structural observations
            Self::UnnamedDefNotLast => "unnamed definition must be last",
        }
    }

    /// Template for custom messages. Contains `{}` placeholder for caller-provided detail.
    pub fn custom_message(&self) -> String {
        match self {
            // Special cases: placeholder embedded in message
            Self::RefCannotHaveChildren => "reference `{}` cannot contain children".to_string(),
            Self::FieldSequenceValue => "field `{}` value must be a single node".to_string(),

            // Cases with backtick-wrapped placeholders
            Self::DuplicateDefinition | Self::UndefinedReference => {
                format!("{}; `{{}}`", self.fallback_message())
            }

            // Cases where custom text differs from fallback
            Self::InvalidTypeAnnotationSyntax => "invalid type annotation; {}".to_string(),
            Self::MixedAltBranches => "mixed alternation; {}".to_string(),

            // Standard pattern: fallback + ": {}"
            _ => format!("{}; {{}}", self.fallback_message()),
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
    pub(crate) range: TextRange,
    pub(crate) message: String,
}

impl RelatedInfo {
    pub fn new(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            range,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiagnosticMessage {
    pub(crate) kind: DiagnosticKind,
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
}

impl DiagnosticMessage {
    pub(crate) fn new(kind: DiagnosticKind, range: TextRange, message: impl Into<String>) -> Self {
        Self {
            kind,
            range,
            suppression_range: range,
            message: message.into(),
            fix: None,
            related: Vec::new(),
        }
    }

    pub(crate) fn with_default_message(kind: DiagnosticKind, range: TextRange) -> Self {
        Self::new(kind, range, kind.fallback_message())
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
                u32::from(related.range.start()),
                u32::from(related.range.end())
            )?;
        }
        Ok(())
    }
}
