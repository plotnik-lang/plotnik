use std::fmt;

use super::rows::{Normalizer, Rows};
use super::schema::{Schema, SchemaTables, Ty};
use super::walk::{Step, Walk, WalkStack};
use crate::LimitExceeded;
use crate::alloc::{Account, Meter, Shared};
use crate::json::{JsonError, Style, Writer};
use crate::tape::{JournalEvent, Tape};

/// One VM match's result graph, stored as flat, indexed rows.
///
/// The VM interprets bytecode, so it keeps results in this logical form
/// instead of generated native types. It shares normalization, validation
/// and rendering with generated publishers.
pub struct Document<'q, 't, 's> {
    schema: Shared<SchemaTables<'q>>,
    rows: Rows<'t>,
    root: u32,
    root_ty: u32,
    source: &'s str,
    account: Account,
    inspection: Option<Tape<'t>>,
}

impl<'q, 't, 's> Document<'q, 't, 's> {
    /// Normalize an accepted tape whose output has type `root_ty`.
    pub fn new(
        meter: &mut Meter<'_>,
        schema: Shared<SchemaTables<'q>>,
        tape: Tape<'t>,
        source: &'s str,
        root_ty: u32,
    ) -> Result<Self, LimitExceeded> {
        let mut rows = Rows::new();
        let root = Normalizer::new().run(meter, &schema.view(), tape.events(), &mut rows)?;
        let mut stack = WalkStack::new();
        let mut walk = Walk::new(
            meter,
            schema.view(),
            &rows,
            source,
            &mut stack,
            root,
            root_ty,
        )?;
        while walk.next(meter)?.is_some() {}

        let mut inspected = false;
        for event in tape.events() {
            meter.charge(1)?;
            inspected |= event.is_inspection();
        }
        Ok(Self {
            schema,
            rows,
            root,
            root_ty,
            source,
            account: meter.account().clone(),
            inspection: inspected.then_some(tape),
        })
    }

    pub fn schema(&self) -> Schema<'_> {
        self.schema.view()
    }

    pub fn inspection_events(&self) -> impl Iterator<Item = JournalEvent<'t>> + '_ {
        self.inspection.iter().flat_map(Tape::events)
    }

    pub fn source(&self) -> &'s str {
        self.source
    }

    /// Walk the result in typed preorder/postorder steps.
    pub fn walk<'w>(
        &'w self,
        meter: &mut Meter<'_>,
        stack: &'w mut WalkStack,
    ) -> Result<Walk<'w, 'w, 't, 's>, LimitExceeded>
    where
        'q: 'w,
    {
        Walk::new(
            meter,
            self.schema.view(),
            &self.rows,
            self.source,
            stack,
            self.root,
            self.root_ty,
        )
    }

    /// The account the document's storage and rendering are charged to.
    pub fn account(&self) -> &Account {
        &self.account
    }

    /// Render the document as JSON, charging the producing operation's fuel.
    pub fn write_json(&self, style: Style, sink: &mut dyn fmt::Write) -> Result<(), JsonError> {
        let mut out = Writer::new(&self.account, style, sink);
        let result = self.render(&mut out);
        out.finish(result)
    }

    fn render(&self, out: &mut Writer<'_, '_>) -> Result<(), JsonError> {
        let schema = self.schema.view();
        let mut stack = WalkStack::new();
        let mut walk = Walk::new(
            out.meter(),
            schema,
            &self.rows,
            self.source,
            &mut stack,
            self.root,
            self.root_ty,
        )?;
        loop {
            let Some(step) = walk.next(out.meter())? else {
                return Ok(());
            };
            match step {
                Step::Node { node, .. } => out.node(node, self.source)?,
                Step::Text { text, .. } => out.string(text)?,
                Step::Bool { value, .. } => out.bool(value)?,
                Step::None { .. } => out.null()?,
                Step::Open { ty, case, .. } => match schema.ty(ty) {
                    Ty::List { .. } => out.open_array()?,
                    Ty::Record { .. } => out.open_object()?,
                    Ty::Variant { .. } => {
                        let case = schema.case(ty, case);
                        out.open_object()?;
                        out.key("$tag")?;
                        out.string(case.name)?;
                        if case.payload().is_some() {
                            out.key("$data")?;
                            out.open_object()?;
                        }
                    }
                    Ty::Option(_) | Ty::Ref(_) => {}
                    Ty::NoValue | Ty::Node | Ty::Text | Ty::Bool => {
                        unreachable!("leaves do not open")
                    }
                },
                Step::Field { record, index } => {
                    out.key(schema.fields(record)[index as usize].name)?;
                }
                Step::Element { .. } => {}
                Step::Close { ty, case } => match schema.ty(ty) {
                    Ty::List { .. } | Ty::Record { .. } => out.close()?,
                    Ty::Variant { .. } => {
                        if schema.case(ty, case).payload().is_some() {
                            out.close()?;
                        }
                        out.close()?;
                    }
                    Ty::Option(_) | Ty::Ref(_) => {}
                    Ty::NoValue | Ty::Node | Ty::Text | Ty::Bool => {
                        unreachable!("leaves do not close")
                    }
                },
            }
        }
    }
}
