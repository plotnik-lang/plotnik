use regex_automata::dfa::dense;
use regex_automata::nfa::thompson;
use regex_syntax::hir::{Class, ClassUnicode, ClassUnicodeRange, Hir, HirKind, Look, Repetition};

pub(crate) fn normalize(pattern: &str) -> Hir {
    let translated = regex_syntax::ParserBuilder::new()
        .utf8(true)
        .octal(false)
        .build()
        .parse(pattern)
        .expect("analyze validates regex syntax before normalization");
    let normalized = normalize_hir(&translated);
    assert!(
        is_normalized(&normalized),
        "regex normalization left target-dependent constructs in pattern {pattern:?}"
    );
    normalized
}

pub(in crate::compiler) fn compile_native_dfa(pattern: &Hir) -> Result<Vec<u8>, String> {
    let dense = build_dense(pattern)?;
    let sparse = dense.to_sparse().map_err(|error| error.to_string())?;
    Ok(sparse.to_bytes_little_endian())
}

fn build_dense(pattern: &Hir) -> Result<dense::DFA<Vec<u32>>, String> {
    let nfa = thompson::NFA::compiler()
        .configure(thompson::NFA::config().which_captures(thompson::WhichCaptures::None))
        .build_from_hir(pattern)
        .map_err(|error| error.to_string())?;
    dense::DFA::builder()
        .configure(
            dense::DFA::config()
                .start_kind(regex_automata::dfa::StartKind::Unanchored)
                .minimize(true),
        )
        .build_from_nfa(&nfa)
        .map_err(|error| error.to_string())
}

fn normalize_hir(hir: &Hir) -> Hir {
    match hir.kind() {
        HirKind::Empty => Hir::empty(),
        HirKind::Literal(literal) => Hir::literal(literal.0.clone()),
        HirKind::Class(Class::Unicode(class)) => Hir::class(Class::Unicode(class.clone())),
        HirKind::Class(Class::Bytes(class)) if class.ranges().is_empty() => Hir::fail(),
        HirKind::Class(Class::Bytes(class)) => {
            let ranges = class.ranges().iter().map(|range| {
                assert!(
                    range.end().is_ascii(),
                    "UTF-8 normalization cannot retain a non-ASCII byte class"
                );
                ClassUnicodeRange::new(char::from(range.start()), char::from(range.end()))
            });
            Hir::class(Class::Unicode(ClassUnicode::new(ranges)))
        }
        HirKind::Look(Look::WordUnicode) => Hir::look(Look::WordAscii),
        HirKind::Look(Look::WordUnicodeNegate) => Hir::look(Look::WordAsciiNegate),
        HirKind::Look(look) => Hir::look(*look),
        HirKind::Repetition(repetition) => Hir::repetition(Repetition {
            sub: Box::new(normalize_hir(&repetition.sub)),
            ..repetition.clone()
        }),
        HirKind::Capture(capture) => normalize_hir(&capture.sub),
        HirKind::Concat(expressions) => {
            Hir::concat(expressions.iter().map(normalize_hir).collect())
        }
        HirKind::Alternation(expressions) => {
            Hir::alternation(expressions.iter().map(normalize_hir).collect())
        }
    }
}

pub(super) fn is_normalized(hir: &Hir) -> bool {
    match hir.kind() {
        HirKind::Empty => true,
        HirKind::Literal(literal) => std::str::from_utf8(&literal.0).is_ok(),
        HirKind::Class(Class::Unicode(_)) => true,
        // regex-syntax canonicalizes the never-match expression to an empty
        // byte class even when its input was Unicode-oriented.
        HirKind::Class(Class::Bytes(class)) => class.ranges().is_empty(),
        HirKind::Look(Look::Start | Look::End | Look::WordAscii | Look::WordAsciiNegate) => true,
        HirKind::Look(_) | HirKind::Capture(_) => false,
        HirKind::Repetition(repetition) => is_normalized(&repetition.sub),
        HirKind::Concat(expressions) | HirKind::Alternation(expressions) => {
            expressions.iter().all(is_normalized)
        }
    }
}
