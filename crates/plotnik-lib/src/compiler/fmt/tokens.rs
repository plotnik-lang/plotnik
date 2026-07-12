use crate::compiler::parse::cst::SyntaxKind;

use super::comments::Comment;
use super::ir::Token;

#[derive(Clone, Copy)]
pub(super) enum Atom<'a> {
    Token(&'a Token),
    Comment(&'a Comment),
}

impl Atom<'_> {
    fn kind(self) -> Option<SyntaxKind> {
        match self {
            Self::Token(token) => Some(token.kind),
            Self::Comment(_) => None,
        }
    }
}

pub(super) fn format_atoms(atoms: &[Atom<'_>]) -> String {
    let mut out = String::new();
    let mut previous: Option<SyntaxKind> = None;
    let mut index = 0;

    while index < atoms.len() {
        let atom = atoms[index];
        if let Atom::Comment(comment) = atom {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(comment.text());
            out.push(' ');
            previous = None;
            index += 1;
            continue;
        }

        let Atom::Token(token) = atom else {
            unreachable!("comments return above")
        };
        if token.kind == SyntaxKind::SingleQuote
            && let Some(Atom::Token(content)) = atoms.get(index + 1).copied()
            && content.kind == SyntaxKind::StringContent
            && let Some(Atom::Token(quote)) = atoms.get(index + 2).copied()
            && quote.kind == SyntaxKind::SingleQuote
            && !content.text().contains('"')
        {
            if needs_space(previous, SyntaxKind::DoubleQuote) {
                out.push(' ');
            }
            out.push('"');
            out.push_str(content.text());
            out.push('"');
            previous = Some(SyntaxKind::DoubleQuote);
            index += 3;
            continue;
        }

        if needs_space(previous, token.kind) {
            out.push(' ');
        }
        out.push_str(token.text());
        previous = atom.kind();
        index += 1;
    }
    out.trim_end().to_owned()
}

pub(super) fn needs_space(previous: Option<SyntaxKind>, current: SyntaxKind) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    let previous = TokenRole::of(previous);
    let current = TokenRole::of(current);
    if current == TokenRole::Close || previous == TokenRole::Open {
        return false;
    }
    if current == TokenRole::PredicateOperator || previous == TokenRole::PredicateOperator {
        return true;
    }
    if matches!(
        current,
        TokenRole::Colon | TokenRole::Refinement | TokenRole::Quantifier
    ) || matches!(previous, TokenRole::Refinement | TokenRole::PrefixOperator)
    {
        return false;
    }
    if current == TokenRole::StringContent && previous == TokenRole::Quote {
        return false;
    }
    if current == TokenRole::Quote && previous == TokenRole::StringContent {
        return false;
    }
    if current == TokenRole::TypeSeparator || previous == TokenRole::TypeSeparator {
        return true;
    }
    if previous == TokenRole::Colon {
        return true;
    }
    if current == TokenRole::DefinitionSeparator || previous == TokenRole::DefinitionSeparator {
        return true;
    }
    if current == TokenRole::Capture {
        return true;
    }
    true
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenRole {
    Open,
    Close,
    Colon,
    Refinement,
    PrefixOperator,
    Quantifier,
    Quote,
    StringContent,
    TypeSeparator,
    PredicateOperator,
    DefinitionSeparator,
    Capture,
    Other,
}

impl TokenRole {
    fn of(kind: SyntaxKind) -> Self {
        match kind {
            SyntaxKind::ParenOpen | SyntaxKind::BracketOpen | SyntaxKind::BraceOpen => Self::Open,
            SyntaxKind::ParenClose | SyntaxKind::BracketClose | SyntaxKind::BraceClose => {
                Self::Close
            }
            SyntaxKind::Colon => Self::Colon,
            SyntaxKind::Slash | SyntaxKind::Hash => Self::Refinement,
            SyntaxKind::Minus | SyntaxKind::Negation => Self::PrefixOperator,
            kind if is_quantifier(kind) => Self::Quantifier,
            SyntaxKind::DoubleQuote | SyntaxKind::SingleQuote => Self::Quote,
            SyntaxKind::StringContent => Self::StringContent,
            SyntaxKind::DoubleColon => Self::TypeSeparator,
            kind if is_predicate_operator(kind) => Self::PredicateOperator,
            SyntaxKind::Equals => Self::DefinitionSeparator,
            SyntaxKind::CaptureToken | SyntaxKind::SuppressiveCapture => Self::Capture,
            _ => Self::Other,
        }
    }
}

pub(super) fn is_quantifier(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Star
            | SyntaxKind::Plus
            | SyntaxKind::Question
            | SyntaxKind::StarQuestion
            | SyntaxKind::PlusQuestion
            | SyntaxKind::QuestionQuestion
    )
}

fn is_predicate_operator(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::OpEq
            | SyntaxKind::OpNe
            | SyntaxKind::OpStartsWith
            | SyntaxKind::OpEndsWith
            | SyntaxKind::OpContains
            | SyntaxKind::OpRegexMatch
            | SyntaxKind::OpRegexNoMatch
    )
}
