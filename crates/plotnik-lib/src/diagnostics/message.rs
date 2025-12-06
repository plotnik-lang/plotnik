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
    ErrorTakesNoArguments,
    RefCannotHaveChildren,
    ErrorMissingOutsideParens,
    UnsupportedPredicate,
    UnexpectedToken,

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
        // All current diagnostics are errors.
        // Naming conventions could become warnings in a lenient mode.
        Severity::Error
    }

    /// Whether this kind suppresses `other` when spans overlap.
    ///
    /// Uses enum discriminant ordering: lower position = higher priority.
    /// A higher-priority diagnostic suppresses lower-priority ones in the same span.
    pub fn suppresses(&self, other: &DiagnosticKind) -> bool {
        self < other
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
    pub(crate) severity: Severity,
    pub(crate) range: TextRange,
    pub(crate) message: String,
    pub(crate) fix: Option<Fix>,
    pub(crate) related: Vec<RelatedInfo>,
}

impl DiagnosticMessage {
    pub(crate) fn error(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            range,
            message: message.into(),
            fix: None,
            related: Vec::new(),
        }
    }

    pub(crate) fn warning(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            range,
            message: message.into(),
            fix: None,
            related: Vec::new(),
        }
    }

    pub(crate) fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    pub(crate) fn is_warning(&self) -> bool {
        self.severity == Severity::Warning
    }
}

impl std::fmt::Display for DiagnosticMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} at {}..{}: {}",
            self.severity,
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
