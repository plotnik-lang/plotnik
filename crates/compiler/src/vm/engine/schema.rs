use plotnik_runtime::{Case, Field, LimitExceeded, Member, Meter, SchemaTables, Shared, Span, Ty};

use crate::bytecode::{Module, TypeDefKind, TypeKind};

pub(super) fn build<'q>(
    meter: &mut Meter<'_>,
    module: &'q Module,
) -> Result<Shared<SchemaTables<'q>>, LimitExceeded> {
    let types = module.types();
    let strings = module.strings();
    let mut schema = SchemaTables::new();
    for _ in types.members() {
        schema.members.push(meter, Member::UNUSED)?;
    }
    for (owner, definition) in types.iter().enumerate() {
        meter.charge(1)?;
        let shape = match definition.decode() {
            TypeDefKind::Primitive(kind) => match kind {
                TypeKind::Node => Ty::Node,
                TypeKind::NoValue => Ty::NoValue,
                TypeKind::Text => Ty::Text,
                TypeKind::Bool => Ty::Bool,
                _ => unreachable!("a primitive definition has a primitive kind"),
            },
            TypeDefKind::Wrapper { kind, inner } => {
                let inner = u32::from(u16::from(inner));
                match kind {
                    TypeKind::Option => Ty::Option(inner),
                    TypeKind::ListZeroOrMore | TypeKind::ListOneOrMore => Ty::List {
                        element: inner,
                        minimum: u32::from(kind == TypeKind::ListOneOrMore),
                    },
                    TypeKind::Alias => Ty::Ref(inner),
                    _ => unreachable!("a wrapper definition has a wrapper kind"),
                }
            }
            TypeDefKind::Record {
                member_start,
                member_count,
            } => {
                let start = schema.fields.len() as u32;
                for index in 0..member_count {
                    let id = member_start as usize + index as usize;
                    let member = types.get_member(id);
                    schema.members[id] = Member::new(owner as u32, u32::from(index));
                    schema.fields.push(
                        meter,
                        Field {
                            name: strings.get(member.name_id),
                            ty: u32::from(u16::from(member.type_id)),
                        },
                    )?;
                }
                Ty::Record {
                    fields: Span::new(start, u32::from(member_count)),
                }
            }
            TypeDefKind::Variant {
                member_start,
                member_count,
            } => {
                let start = schema.cases.len() as u32;
                for index in 0..member_count {
                    let id = member_start as usize + index as usize;
                    let member = types.get_member(id);
                    schema.members[id] = Member::new(owner as u32, u32::from(index));
                    let payload = u32::from(u16::from(member.type_id));
                    schema.cases.push(
                        meter,
                        Case {
                            name: strings.get(member.name_id),
                            payload: if payload == 0 {
                                Case::NO_PAYLOAD
                            } else {
                                payload
                            },
                        },
                    )?;
                }
                Ty::Variant {
                    cases: Span::new(start, u32::from(member_count)),
                }
            }
        };
        schema.types.push(meter, shape)?;
    }
    Shared::new(meter, schema)
}
