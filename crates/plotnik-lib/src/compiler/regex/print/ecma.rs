//! ECMAScript spelling for compiler-normalized regex HIR.

use std::fmt::Write as _;

use regex_syntax::hir::{Class, Hir, HirKind, Literal, Look, Repetition};

/// A pattern source and the only flag generated ECMAScript may enable.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct EcmaRegex {
    pub(crate) source: String,
    pub(crate) flags: &'static str,
}

/// Print normalized HIR. Every accepted query is representable; encountering
/// another HIR node is an internal normalization bug, not a target diagnostic.
pub(crate) fn print(pattern: &Hir) -> EcmaRegex {
    let mut source = String::new();
    write_hir(pattern, Precedence::Alternation, &mut source);
    EcmaRegex { source, flags: "u" }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Precedence {
    Alternation,
    Concatenation,
    Repetition,
    Atom,
}

fn write_hir(pattern: &Hir, parent: Precedence, out: &mut String) {
    let precedence = precedence(pattern);
    let grouped = precedence < parent;
    if grouped {
        out.push_str("(?:");
    }

    match pattern.kind() {
        HirKind::Empty => out.push_str("(?:)"),
        HirKind::Literal(literal) => write_literal(literal, out),
        HirKind::Class(class) => write_class(class, out),
        HirKind::Look(Look::Start) => out.push('^'),
        HirKind::Look(Look::End) => out.push('$'),
        HirKind::Look(Look::WordAscii) => out.push_str(r"\b"),
        HirKind::Look(Look::WordAsciiNegate) => out.push_str(r"\B"),
        HirKind::Look(_) => unreachable!("normalization removes non-portable assertions"),
        HirKind::Repetition(repetition) => write_repetition(repetition, out),
        HirKind::Capture(_) => unreachable!("normalization removes capture groups"),
        HirKind::Concat(expressions) => {
            for expression in expressions {
                write_hir(expression, Precedence::Concatenation, out);
            }
        }
        HirKind::Alternation(expressions) => {
            for (index, expression) in expressions.iter().enumerate() {
                if index > 0 {
                    out.push('|');
                }
                write_hir(expression, Precedence::Alternation, out);
            }
        }
    }

    if grouped {
        out.push(')');
    }
}

fn precedence(pattern: &Hir) -> Precedence {
    match pattern.kind() {
        HirKind::Alternation(_) => Precedence::Alternation,
        HirKind::Concat(_) => Precedence::Concatenation,
        HirKind::Repetition(_) => Precedence::Repetition,
        HirKind::Literal(literal) if literal_chars(literal).count() > 1 => {
            Precedence::Concatenation
        }
        HirKind::Empty
        | HirKind::Literal(_)
        | HirKind::Class(_)
        | HirKind::Look(_)
        | HirKind::Capture(_) => Precedence::Atom,
    }
}

fn write_repetition(repetition: &Repetition, out: &mut String) {
    if repetition_operand_is_atom(&repetition.sub) {
        write_hir(&repetition.sub, Precedence::Atom, out);
    } else {
        out.push_str("(?:");
        write_hir(&repetition.sub, Precedence::Alternation, out);
        out.push(')');
    }
    match (repetition.min, repetition.max) {
        (0, Some(1)) => out.push('?'),
        (0, None) => out.push('*'),
        (1, None) => out.push('+'),
        (min, Some(max)) if min == max => {
            write!(out, "{{{min}}}").expect("writing regex source to a String is infallible")
        }
        (min, Some(max)) => {
            write!(out, "{{{min},{max}}}").expect("writing regex source to a String is infallible")
        }
        (min, None) => {
            write!(out, "{{{min},}}").expect("writing regex source to a String is infallible")
        }
    }
    if !repetition.greedy {
        out.push('?');
    }
}

fn repetition_operand_is_atom(pattern: &Hir) -> bool {
    match pattern.kind() {
        HirKind::Empty | HirKind::Class(_) => true,
        HirKind::Literal(literal) => literal_chars(literal).count() == 1,
        HirKind::Look(_)
        | HirKind::Repetition(_)
        | HirKind::Capture(_)
        | HirKind::Concat(_)
        | HirKind::Alternation(_) => false,
    }
}

fn write_literal(literal: &Literal, out: &mut String) {
    for character in literal_chars(literal) {
        if literal_is_plain(character) {
            out.push(character);
            continue;
        }
        if character.is_ascii() && r"\.^$|?*+()[]{}/".contains(character) {
            out.push('\\');
            out.push(character);
            continue;
        }
        write_code_point(character, out);
    }
}

fn literal_chars(literal: &Literal) -> impl Iterator<Item = char> + '_ {
    std::str::from_utf8(&literal.0)
        .expect("normalized literals contain complete UTF-8 scalar values")
        .chars()
}

fn literal_is_plain(character: char) -> bool {
    !character.is_control()
        && !matches!(character, '\u{2028}' | '\u{2029}')
        && (character as u32) <= 0xFFFF
        && !r"\.^$|?*+()[]{}/".contains(character)
}

fn write_class(class: &Class, out: &mut String) {
    let Class::Unicode(class) = class else {
        let Class::Bytes(class) = class else {
            unreachable!("regex-syntax has only Unicode and byte classes")
        };
        assert!(
            class.ranges().is_empty(),
            "normalization retains byte classes only for never-match"
        );
        out.push_str("[]");
        return;
    };

    out.push('[');
    for range in class.ranges() {
        if range.start() <= '\u{D7FF}' && range.end() >= '\u{E000}' {
            write_class_range(range.start(), '\u{D7FF}', out);
            write_class_range('\u{E000}', range.end(), out);
            continue;
        }
        write_class_range(range.start(), range.end(), out);
    }
    out.push(']');
}

fn write_class_range(start: char, end: char, out: &mut String) {
    write_class_character(start, out);
    if start == end {
        return;
    }
    out.push('-');
    write_class_character(end, out);
}

fn write_class_character(character: char, out: &mut String) {
    if character.is_ascii_alphanumeric() || character == '_' {
        out.push(character);
        return;
    }
    write_code_point(character, out);
}

fn write_code_point(character: char, out: &mut String) {
    write!(out, "\\u{{{:x}}}", character as u32)
        .expect("writing regex source to a String is infallible");
}
