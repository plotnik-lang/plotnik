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
    // === Unclosed delimiters (highest priority) ===
    // These cause cascading errors throughout the rest of the file
    UnclosedTree,
    UnclosedSequence,
    UnclosedAlternation,

    // === Expected token errors ===
    // User omitted something required - root cause errors
    ExpectedExpression,
    ExpectedTypeName,
    ExpectedCaptureName,
    ExpectedFieldName,
    ExpectedSubtype,

    // === Invalid token/syntax usage ===
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

    // === Naming validation ===
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

    // === Semantic errors ===
    // Valid syntax, invalid semantics
    DuplicateDefinition,
    UndefinedReference,
    MixedAltBranches,
    RecursionNoEscape,
    FieldSequenceValue,

    // === Structural observations (lowest priority) ===
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

    /// Default message for this diagnostic kind.
    ///
    /// Provides a sensible fallback; callers can override with context-specific messages.
    pub fn default_message(&self) -> &'static str {
        match self {
            // Unclosed delimiters
            Self::UnclosedTree => "unclosed tree; expected ')'",
            Self::UnclosedSequence => "unclosed sequence; expected '}'",
            Self::UnclosedAlternation => "unclosed alternation; expected ']'",

            // Expected token errors
            Self::ExpectedExpression => "expected expression",
            Self::ExpectedTypeName => "expected type name after '::'",
            Self::ExpectedCaptureName => "expected capture name after '@'",
            Self::ExpectedFieldName => "expected field name",
            Self::ExpectedSubtype => "expected subtype after '/'",

            // Invalid token/syntax usage
            Self::EmptyTree => "empty tree expression - expected node type or children",
            Self::BareIdentifier => {
                "bare identifier not allowed; nodes must be enclosed in parentheses"
            }
            Self::InvalidSeparator => "invalid separator; plotnik uses whitespace for separation",
            Self::InvalidFieldEquals => "'=' is not valid for field constraints; use ':'",
            Self::InvalidSupertypeSyntax => "references cannot use supertype syntax (/)",
            Self::InvalidTypeAnnotationSyntax => "invalid type annotation syntax",
            Self::ErrorTakesNoArguments => "(ERROR) takes no arguments",
            Self::RefCannotHaveChildren => "reference cannot contain children",
            Self::ErrorMissingOutsideParens => {
                "ERROR and MISSING must be inside parentheses: (ERROR) or (MISSING ...)"
            }
            Self::UnsupportedPredicate => {
                "tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported"
            }
            Self::UnexpectedToken => "unexpected token",
            Self::CaptureWithoutTarget => "capture '@' must follow an expression to capture",
            Self::LowercaseBranchLabel => {
                "tagged alternation labels must be Capitalized (they map to enum variants)"
            }

            // Naming validation
            Self::CaptureNameHasDots => "capture names cannot contain dots",
            Self::CaptureNameHasHyphens => "capture names cannot contain hyphens",
            Self::CaptureNameUppercase => "capture names must start with lowercase",
            Self::DefNameLowercase => "definition names must start with uppercase",
            Self::DefNameHasSeparators => "definition names cannot contain separators",
            Self::BranchLabelHasSeparators => "branch labels cannot contain separators",
            Self::FieldNameHasDots => "field names cannot contain dots",
            Self::FieldNameHasHyphens => "field names cannot contain hyphens",
            Self::FieldNameUppercase => "field names must start with lowercase",
            Self::TypeNameInvalidChars => "type names cannot contain dots or hyphens",

            // Semantic errors
            Self::DuplicateDefinition => "duplicate definition",
            Self::UndefinedReference => "undefined reference",
            Self::MixedAltBranches => "mixed tagged and untagged branches in alternation",
            Self::RecursionNoEscape => "recursive pattern can never match: no escape path",
            Self::FieldSequenceValue => "field value must match a single node, not a sequence",

            // Structural observations
            Self::UnnamedDefNotLast => "unnamed definition must be last in file",
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
    pub(crate) range: TextRange,
    pub(crate) message: String,
    pub(crate) fix: Option<Fix>,
    pub(crate) related: Vec<RelatedInfo>,
}

impl DiagnosticMessage {
    pub(crate) fn new(kind: DiagnosticKind, range: TextRange, message: impl Into<String>) -> Self {
        Self {
            kind,
            range,
            message: message.into(),
            fix: None,
            related: Vec::new(),
        }
    }

    pub(crate) fn with_default_message(kind: DiagnosticKind, range: TextRange) -> Self {
        Self::new(kind, range, kind.default_message())
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
