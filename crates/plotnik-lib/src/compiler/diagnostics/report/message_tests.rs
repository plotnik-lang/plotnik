//! Tests for `DiagnosticKind` and `Severity`: default severities, cascade
//! suppression ordering, and message/template rendering.

use super::*;

#[test]
fn severity_display() {
    insta::assert_snapshot!(format!("{}", Severity::Error), @"error");
    insta::assert_snapshot!(format!("{}", Severity::Warning), @"warning");
}

#[test]
fn diagnostic_kind_default_severity() {
    assert_eq!(DiagnosticKind::UnclosedTree.severity(), Severity::Error);
    assert_eq!(DiagnosticKind::MissingDefName.severity(), Severity::Error);
}

#[test]
fn diagnostic_kind_suppression_order() {
    // Higher priority (earlier in enum) suppresses lower priority (later in enum)
    assert!(DiagnosticKind::UnclosedTree.suppresses(&DiagnosticKind::MissingDefName));
    assert!(DiagnosticKind::UnclosedTree.suppresses(&DiagnosticKind::UndefinedReference));
    assert!(DiagnosticKind::ExpectedExpression.suppresses(&DiagnosticKind::MissingDefName));

    // Same kind doesn't suppress itself
    assert!(!DiagnosticKind::UnclosedTree.suppresses(&DiagnosticKind::UnclosedTree));

    // Lower priority doesn't suppress higher priority
    assert!(!DiagnosticKind::MissingDefName.suppresses(&DiagnosticKind::UnclosedTree));
}

#[test]
fn diagnostic_kind_fallback_messages() {
    assert_eq!(
        DiagnosticKind::UnclosedTree.summary(),
        "missing closing `)`"
    );
    assert_eq!(
        DiagnosticKind::UnclosedSequence.summary(),
        "missing closing `}`"
    );
    assert_eq!(
        DiagnosticKind::UnclosedAlternation.summary(),
        "missing closing `]`"
    );
    assert_eq!(
        DiagnosticKind::ExpectedExpression.summary(),
        "expected an expression"
    );
}

#[test]
fn diagnostic_kind_custom_messages() {
    // Detail replaces the whole message
    assert_eq!(DiagnosticKind::UnexpectedToken.template(), "{}");
    // Detail is interpolated into a template
    assert_eq!(
        DiagnosticKind::UndefinedReference.template(),
        "`{}` is not defined"
    );
    // Default: fallback message plus detail
    assert_eq!(
        DiagnosticKind::DuplicateCaptureInScope.template(),
        "capture `@{}` already defined in this scope"
    );
}

#[test]
fn diagnostic_kind_message_rendering() {
    // No custom message → fallback
    assert_eq!(
        DiagnosticKind::UnclosedTree.render(None),
        "missing closing `)`"
    );
    // With custom message → template applied
    assert_eq!(
        DiagnosticKind::UnexpectedToken.render(Some("expected `)`")),
        "expected `)`"
    );
    assert_eq!(
        DiagnosticKind::UndefinedReference.render(Some("Foo")),
        "`Foo` is not defined"
    );
}
