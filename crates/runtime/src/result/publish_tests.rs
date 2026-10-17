use std::alloc::Layout;

use crate::json::{Fields, Json, Shape, Style};
use crate::{
    Account, Document, Field, JournalEvent, LimitExceeded, Member, Meter, Publish, Publisher,
    ResolvedRuntimeLimits, Schema, Scope, Span, Step, Tape, Ty, Walk,
};

#[derive(Clone, Copy)]
struct List<'r> {
    values: &'r [bool],
}

crate::result_family!(List<'r>);

impl Json for List<'_> {
    fn shape(&self) -> Shape<'_> {
        Shape::Record(self)
    }
}

impl Fields for List<'_> {
    fn count(&self) -> usize {
        1
    }

    fn field(&self, index: usize) -> (&'static str, &dyn Json) {
        assert_eq!(index, 0);
        ("values", &self.values)
    }
}

impl<'t, 's> Publish<'t, 's> for List<'_> {
    const SCHEMA: Schema<'static> = Schema {
        types: &[
            Ty::Record {
                fields: Span::new(0, 1),
            },
            Ty::List {
                element: 2,
                minimum: 0,
            },
            Ty::Bool,
        ],
        fields: &[Field {
            name: "values",
            ty: 1,
        }],
        cases: &[],
        members: &[Member::new(0, 0)],
    };
    const ROOT: u32 = 0;
    const LAYOUTS: &'static [Layout] = &[
        Layout::new::<Self>(),
        Layout::new::<&[bool]>(),
        Layout::new::<bool>(),
    ];

    fn build<'a>(
        walk: &mut Walk<'_, '_, 't, 's>,
        scope: &Scope<'a>,
        meter: &mut Meter<'_>,
    ) -> Result<Self::Borrowed<'a>, LimitExceeded>
    where
        Self: 'a,
    {
        let mut writer = None;
        let mut pending = None;
        let mut result = None;
        while let Some(step) = walk.next(meter)? {
            match step {
                Step::Open { ty: 1, len, .. } => writer = Some(scope.slice::<bool>(len)),
                Step::Bool { value, .. } => pending = Some(value),
                Step::Element { .. } => writer
                    .as_mut()
                    .unwrap()
                    .write(meter, pending.take().unwrap())?,
                Step::Close { ty: 1, .. } => {
                    result = Some(List {
                        values: writer.take().unwrap().finish(),
                    })
                }
                _ => {}
            }
        }
        Ok(result.unwrap())
    }

    fn json<'a>(value: &'a Self::Borrowed<'a>) -> &'a dyn Json
    where
        Self: 'a,
    {
        value
    }
}

fn tape(meter: &mut Meter<'_>) -> Tape<'static> {
    let mut tape = Tape::new();
    for event in [
        JournalEvent::RecordOpen,
        JournalEvent::RecordSet(0),
        JournalEvent::ListOpen,
        JournalEvent::BoolValue(true),
        JournalEvent::ArrayPush,
        JournalEvent::BoolValue(false),
        JournalEvent::ArrayPush,
        JournalEvent::BoolValue(true),
        JournalEvent::ArrayPush,
        JournalEvent::ListClose,
        JournalEvent::RecordClose,
    ] {
        tape.push(meter, event).unwrap();
    }
    tape
}

#[test]
fn typed_and_document_publication_render_the_same_list_contract() {
    let account = Account::new(ResolvedRuntimeLimits {
        fuel_limit: None,
        max_memory: None,
    })
    .unwrap();
    let baseline = account.memory_used();

    {
        let mut meter = account.meter();
        let tape = tape(&mut meter);
        let typed = Publisher::new()
            .publish::<List<'_>>(&mut meter, &tape, "")
            .unwrap();
        let mut schema = crate::SchemaTables::new();
        schema
            .types
            .extend_from_slice(&mut meter, List::SCHEMA.types)
            .unwrap();
        schema
            .fields
            .extend_from_slice(&mut meter, List::SCHEMA.fields)
            .unwrap();
        schema
            .members
            .extend_from_slice(&mut meter, List::SCHEMA.members)
            .unwrap();
        let schema = crate::Shared::new(&mut meter, schema).unwrap();
        let document = Document::new(&mut meter, schema, tape, "", 0).unwrap();
        drop(meter);

        assert_eq!(typed.view().values, [true, false, true]);
        let mut json = String::new();
        typed.write_json("", Style::COMPACT, &mut json).unwrap();
        assert_eq!(json, r#"{"values":[true,false,true]}"#);
        json.clear();
        document.write_json(Style::COMPACT, &mut json).unwrap();
        assert_eq!(json, r#"{"values":[true,false,true]}"#);
        drop(typed);
        drop(document);
    }

    assert_eq!(account.memory_used(), baseline);
}

#[test]
fn fuel_exhaustion_during_publication_releases_partial_storage() {
    let input = Account::new(ResolvedRuntimeLimits {
        fuel_limit: None,
        max_memory: None,
    })
    .unwrap();
    let tape = tape(&mut input.meter());
    let account = Account::new(ResolvedRuntimeLimits {
        fuel_limit: None,
        max_memory: None,
    })
    .unwrap();
    let result = Publisher::new()
        .publish::<List<'_>>(&mut account.meter(), &tape, "")
        .unwrap();
    assert_eq!(result.view().values, [true, false, true]);
    drop(result);
    let needed = account.fuel_used();

    for limit in 0..=needed {
        let account = Account::new(ResolvedRuntimeLimits {
            fuel_limit: Some(limit),
            max_memory: None,
        })
        .unwrap();
        let baseline = account.memory_used();

        let result = Publisher::new().publish::<List<'_>>(&mut account.meter(), &tape, "");

        if limit < needed {
            assert!(matches!(result, Err(LimitExceeded::OutOfFuel(n)) if n == limit));
        } else {
            assert_eq!(result.as_ref().unwrap().view().values, [true, false, true]);
        }
        drop(result);
        assert_eq!(account.memory_used(), baseline);
    }
}

fn render_protocol(events: &[JournalEvent<'static>], root: u32) -> String {
    let account = Account::new(ResolvedRuntimeLimits {
        fuel_limit: None,
        max_memory: None,
    })
    .unwrap();
    let mut meter = account.meter();
    let mut schema = crate::SchemaTables::new();
    schema
        .types
        .extend_from_slice(
            &mut meter,
            &[
                Ty::Record {
                    fields: Span::new(0, 3),
                },
                Ty::Option(3),
                Ty::List {
                    element: 3,
                    minimum: 1,
                },
                Ty::Bool,
                Ty::Variant {
                    cases: Span::new(0, 1),
                },
            ],
        )
        .unwrap();
    schema
        .fields
        .extend_from_slice(
            &mut meter,
            &[
                Field {
                    name: "maybe",
                    ty: 1,
                },
                Field {
                    name: "values",
                    ty: 2,
                },
                Field {
                    name: "flag",
                    ty: 3,
                },
            ],
        )
        .unwrap();
    schema
        .cases
        .push(
            &mut meter,
            crate::Case {
                name: "Case",
                payload: 0,
            },
        )
        .unwrap();
    schema
        .members
        .extend_from_slice(
            &mut meter,
            &[
                Member::new(0, 0),
                Member::new(0, 1),
                Member::new(0, 2),
                Member::new(4, 0),
            ],
        )
        .unwrap();
    let schema = crate::Shared::new(&mut meter, schema).unwrap();
    let mut tape = Tape::new();
    for &event in events {
        tape.push(&mut meter, event).unwrap();
    }
    let document = Document::new(&mut meter, schema, tape, "", root).unwrap();
    drop(meter);
    let mut output = String::new();
    document.write_json(Style::COMPACT, &mut output).unwrap();
    output
}

#[test]
fn omitted_lists_and_options_default_but_selected_lists_keep_their_minimum() {
    use JournalEvent::*;

    assert_eq!(
        render_protocol(
            &[RecordOpen, RecordSet(2), BoolValue(false), RecordClose],
            0
        ),
        r#"{"maybe":null,"values":[],"flag":false}"#
    );
    assert_eq!(
        render_protocol(
            &[
                RecordOpen,
                RecordSet(1),
                ListOpen,
                BoolValue(true),
                ArrayPush,
                ListClose,
                RecordSet(2),
                BoolValue(false),
                RecordClose
            ],
            0
        ),
        r#"{"maybe":null,"values":[true],"flag":false}"#
    );
    assert!(
        std::panic::catch_unwind(|| render_protocol(
            &[
                RecordOpen,
                RecordSet(1),
                ListOpen,
                ListClose,
                RecordSet(2),
                BoolValue(false),
                RecordClose
            ],
            0
        ))
        .is_err()
    );
}

#[test]
fn variants_accept_direct_fields_or_completed_payload_but_not_both() {
    use JournalEvent::*;
    let expected = r#"{"$tag":"Case","$data":{"maybe":null,"values":[],"flag":false}}"#;

    assert_eq!(
        render_protocol(
            &[VariantOpen(3), RecordSet(2), BoolValue(false), VariantClose],
            4
        ),
        expected
    );
    assert_eq!(
        render_protocol(
            &[
                VariantOpen(3),
                RecordOpen,
                RecordSet(2),
                BoolValue(false),
                RecordClose,
                VariantClose
            ],
            4
        ),
        expected
    );
    assert!(
        std::panic::catch_unwind(|| render_protocol(
            &[
                VariantOpen(3),
                RecordSet(2),
                BoolValue(true),
                RecordOpen,
                RecordSet(2),
                BoolValue(false),
                RecordClose,
                VariantClose
            ],
            4
        ))
        .is_err()
    );
}

#[test]
fn prefix_protocol_rejects_duplicate_unfinished_and_missing_required_fields() {
    use JournalEvent::*;
    let invalid: &[&[JournalEvent<'static>]] = &[
        &[
            RecordOpen,
            RecordSet(2),
            BoolValue(false),
            RecordSet(2),
            BoolValue(true),
            RecordClose,
        ],
        &[RecordOpen, RecordSet(2), RecordClose],
        &[
            RecordOpen,
            RecordSet(0),
            RecordSet(2),
            BoolValue(false),
            RecordClose,
        ],
        &[RecordOpen, RecordClose],
    ];

    for events in invalid {
        assert!(std::panic::catch_unwind(|| render_protocol(events, 0)).is_err());
    }
}
