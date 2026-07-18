use std::collections::HashSet;

use crate::compiler::ids::TypeId;
use crate::core::Interner;

use super::type_analysis::TypeAnalysisView;
use super::type_shape::TypeShape;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Precedence {
    Union,
    Postfix,
    Atom,
}

struct Description {
    text: String,
    precedence: Precedence,
}

impl Description {
    fn atom(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            precedence: Precedence::Atom,
        }
    }

    fn postfix_operand(self) -> String {
        if self.precedence < Precedence::Postfix {
            return format!("({})", self.text);
        }
        self.text
    }
}

pub(crate) fn describe_type(
    types: &TypeAnalysisView<'_>,
    interner: &Interner,
    type_id: TypeId,
) -> String {
    describe_type_inner(types, interner, type_id, &mut HashSet::new(), 0).text
}

fn describe_type_inner(
    types: &TypeAnalysisView<'_>,
    interner: &Interner,
    type_id: TypeId,
    seen: &mut HashSet<TypeId>,
    depth: usize,
) -> Description {
    const MAX_DEPTH: usize = 4;
    const MAX_MEMBERS: usize = 6;

    if depth == MAX_DEPTH {
        return Description::atom("…");
    }
    if !seen.insert(type_id) {
        return Description::atom("recursive value");
    }

    let shape = types
        .type_shape(type_id)
        .expect("diagnostic type is registered");
    let description = match shape {
        TypeShape::Node => Description::atom("Node"),
        TypeShape::Text => Description::atom("text"),
        TypeShape::Bool => Description::atom("bool"),
        TypeShape::Record(fields) => {
            let mut descriptions = fields
                .iter()
                .take(MAX_MEMBERS)
                .map(|(name, field)| {
                    let field_type =
                        describe_type_inner(types, interner, field.final_type, seen, depth + 1);
                    let name = interner.resolve(*name);
                    format!("{name}: {}", field_type.text)
                })
                .collect::<Vec<_>>();
            if fields.len() > MAX_MEMBERS {
                descriptions.push("…".to_string());
            }
            Description::atom(format!("{{ {} }}", descriptions.join(", ")))
        }
        TypeShape::Variant(cases) => {
            let mut descriptions = cases
                .iter()
                .take(MAX_MEMBERS)
                .map(|(name, payload)| {
                    let name = interner.resolve(*name);
                    payload.type_id().map_or_else(
                        || name.to_string(),
                        |payload| {
                            let payload =
                                describe_type_inner(types, interner, payload, seen, depth + 1);
                            format!("{name}({})", payload.text)
                        },
                    )
                })
                .collect::<Vec<_>>();
            if cases.len() > MAX_MEMBERS {
                descriptions.push("…".to_string());
            }
            Description {
                text: format!("variant {}", descriptions.join(" | ")),
                precedence: Precedence::Union,
            }
        }
        TypeShape::List { element, .. } => {
            let element = describe_type_inner(types, interner, *element, seen, depth + 1);
            Description {
                text: format!("{}[]", element.postfix_operand()),
                precedence: Precedence::Postfix,
            }
        }
        TypeShape::Option(inner) => {
            let inner = describe_type_inner(types, interner, *inner, seen, depth + 1);
            Description {
                text: format!("{} | null", inner.text),
                precedence: Precedence::Union,
            }
        }
        TypeShape::Ref(declaration) => {
            Description::atom(interner.resolve(types.declaration_name(*declaration)))
        }
    };
    seen.remove(&type_id);
    description
}
