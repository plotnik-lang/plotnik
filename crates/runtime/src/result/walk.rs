use tree_sitter::Node;

use super::rows::{Kind, MISSING, Rows, UNTYPED};
use super::schema::{Schema, Ty};
use crate::LimitExceeded;
use crate::alloc::{Buffer, Meter};
use crate::tape::source_text;

/// One step of a typed walk over normalized rows.
///
/// Every value produces either one complete step (`Node`, `Text`, `Bool`,
/// `None`) or an `Open` .. `Close` pair around its children. `ty` is the
/// type of the value's position, so the same row reached through a
/// [`Ty::Ref`] or [`Ty::Option`] position yields the wrapper's steps around
/// the inner value's steps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step<'t, 's> {
    Node {
        ty: u32,
        node: Node<'t>,
    },
    Text {
        ty: u32,
        text: &'s str,
    },
    Bool {
        ty: u32,
        value: bool,
    },
    /// An absent option.
    None {
        ty: u32,
    },
    /// A composite begins. `len` is the child count: 1 for a present option
    /// or a reference, the element count of a list, the field count of a
    /// record or of a variant case. `case` is the variant case, else 0.
    Open {
        ty: u32,
        len: usize,
        case: u32,
    },
    /// The next child fills field `index` of record type `record` (for a
    /// variant, of its case payload record).
    Field {
        record: u32,
        index: u32,
    },
    /// The value that just completed is the next element of list `list`.
    Element {
        list: u32,
    },
    /// The innermost composite is complete.
    Close {
        ty: u32,
        case: u32,
    },
}

#[derive(Clone, Copy)]
struct Frame {
    ty: u32,
    row: u32,
    /// Next child to visit; `u32::MAX` before the value opened.
    next: u32,
    /// Edge start of a composite's children.
    edges: u32,
    /// Child count of a composite.
    len: u32,
    /// Variant case, or the payload record type when walking case fields.
    case: u32,
}

const UNOPENED: u32 = u32::MAX;

/// Reusable scratch of a [`Walk`].
pub struct WalkStack {
    frames: Buffer<Frame>,
}

impl WalkStack {
    pub fn new() -> Self {
        Self {
            frames: Buffer::new(),
        }
    }
}

impl Default for WalkStack {
    fn default() -> Self {
        Self::new()
    }
}

/// An iterative, typed preorder/postorder walk of one result.
///
/// The walk applies the schema rules every consumer shares: field order,
/// an unset or absent option is `None`, an unset list is empty, and every
/// other field must be set. It panics on a rule violation, because only a
/// verified matcher produces the rows. Native stack use does not depend on
/// result depth; the frame stack grows on the heap under the meter.
pub struct Walk<'w, 'r, 't, 's> {
    schema: Schema<'w>,
    rows: &'r Rows<'t>,
    source: &'s str,
    stack: &'w mut WalkStack,
    /// A step to return before resuming the frame stack.
    queued: Option<Step<'t, 's>>,
}

impl<'w, 'r, 't, 's> Walk<'w, 'r, 't, 's> {
    pub fn new(
        meter: &mut Meter<'_>,
        schema: Schema<'w>,
        rows: &'r Rows<'t>,
        source: &'s str,
        stack: &'w mut WalkStack,
        root: u32,
        root_ty: u32,
    ) -> Result<Self, LimitExceeded> {
        stack.frames.clear();
        stack.frames.push(meter, Frame::unopened(root_ty, root))?;
        Ok(Self {
            schema,
            rows,
            source,
            stack,
            queued: None,
        })
    }

    pub fn schema(&self) -> Schema<'w> {
        self.schema
    }

    /// The next step, or `None` once the root closed.
    pub fn next(&mut self, meter: &mut Meter<'_>) -> Result<Option<Step<'t, 's>>, LimitExceeded> {
        if let Some(step) = self.queued.take() {
            return Ok(Some(step));
        }
        let Some(frame) = self.stack.frames.last().copied() else {
            return Ok(None);
        };
        meter.charge(1)?;

        if frame.next == UNOPENED {
            return self.open(meter, frame).map(Some);
        }
        self.resume(meter, frame).map(Some)
    }

    /// First visit of a value: leaves complete at once, composites open.
    fn open(&mut self, meter: &mut Meter<'_>, frame: Frame) -> Result<Step<'t, 's>, LimitExceeded> {
        let ty = frame.ty;
        let row = (frame.row != MISSING).then(|| self.rows.row(frame.row));
        let shape = self.schema.ty(ty);

        let leaf = match shape {
            Ty::NoValue => {
                expect_kind(row, Kind::Node, ty);
                Some(Step::None { ty })
            }
            Ty::Node => {
                let row = expect_kind(row, Kind::Node, ty);
                Some(Step::Node {
                    ty,
                    node: self.rows.node(row.a),
                })
            }
            Ty::Text => {
                let row = expect_kind(row, Kind::Text, ty);
                Some(Step::Text {
                    ty,
                    text: source_text(self.source, row.a as usize..row.b as usize),
                })
            }
            Ty::Bool => {
                let row = expect_kind(row, Kind::Bool, ty);
                Some(Step::Bool {
                    ty,
                    value: row.a != 0,
                })
            }
            Ty::Option(_) if row.is_none_or(|row| row.kind == Kind::Absent) => {
                Some(Step::None { ty })
            }
            _ => None,
        };
        if let Some(step) = leaf {
            self.complete(meter)?;
            return Ok(step);
        }

        let (edges, len, case) = match shape {
            Ty::Option(_) | Ty::Ref(_) => (0, 1, 0),
            Ty::List { minimum, .. } => match row {
                None => (0, 0, 0),
                Some(row) => {
                    let row = expect_kind(Some(row), Kind::List, ty);
                    assert!(
                        row.b >= minimum,
                        "a selected list has fewer than {minimum} elements"
                    );
                    (row.a, row.b, 0)
                }
            },
            Ty::Record { fields } => {
                let row = expect_kind(row, Kind::Record, ty);
                if row.b == UNTYPED {
                    (MISSING, fields.len, 0)
                } else {
                    assert_eq!(
                        row.b, ty,
                        "a record of type {} fills a {ty} position",
                        row.b
                    );
                    (row.a, row.c, 0)
                }
            }
            Ty::Variant { .. } => {
                let row = expect_kind(row, Kind::Variant, ty);
                assert_eq!(
                    row.b, ty,
                    "a variant of type {} fills a {ty} position",
                    row.b
                );
                let len = self.schema.case_fields(ty, row.c).len() as u32;
                (row.a, len, row.c)
            }
            Ty::NoValue | Ty::Node | Ty::Text | Ty::Bool => unreachable!("leaves completed above"),
        };

        let top = self.top();
        top.next = 0;
        top.edges = edges;
        top.len = len;
        top.case = case;
        Ok(Step::Open {
            ty,
            len: len as usize,
            case,
        })
    }

    /// Later visits of an open composite: descend into the next child or close.
    fn resume(
        &mut self,
        meter: &mut Meter<'_>,
        frame: Frame,
    ) -> Result<Step<'t, 's>, LimitExceeded> {
        let ty = frame.ty;
        let shape = self.schema.ty(ty);

        if frame.next == frame.len {
            self.complete(meter)?;
            return Ok(Step::Close {
                ty,
                case: frame.case,
            });
        }

        let next = frame.next;
        self.top().next = next + 1;
        match shape {
            Ty::Option(inner) | Ty::Ref(inner) => {
                self.descend(meter, inner, frame.row)?;
                self.next(meter)
                    .map(|step| step.expect("a child step follows"))
            }
            Ty::List { element, .. } => {
                let child = self.rows.edges(frame.edges, frame.len)[next as usize];
                self.descend(meter, element, child)?;
                self.next(meter)
                    .map(|step| step.expect("a child step follows"))
            }
            Ty::Record { .. } | Ty::Variant { .. } => {
                let record = match shape {
                    Ty::Record { .. } => ty,
                    _ => self
                        .schema
                        .case(ty, frame.case)
                        .payload()
                        .expect("a case with fields has a payload record"),
                };
                let fields = self.schema.fields(record);
                let child = if frame.edges == MISSING {
                    MISSING
                } else {
                    self.rows.edges(frame.edges, frame.len)[next as usize]
                };
                let field_ty = fields[next as usize].ty;
                if child == MISSING {
                    assert!(
                        matches!(self.schema.ty(field_ty), Ty::Option(_) | Ty::List { .. }),
                        "required field `{}` of result type {ty} is unset",
                        fields[next as usize].name
                    );
                }
                self.descend(meter, field_ty, child)?;
                Ok(Step::Field {
                    record,
                    index: next,
                })
            }
            Ty::NoValue | Ty::Node | Ty::Text | Ty::Bool => unreachable!("leaves have no children"),
        }
    }

    fn descend(&mut self, meter: &mut Meter<'_>, ty: u32, row: u32) -> Result<(), LimitExceeded> {
        self.stack.frames.push(meter, Frame::unopened(ty, row))
    }

    /// Pop the finished value; queue the list-element step its parent needs.
    fn complete(&mut self, _meter: &mut Meter<'_>) -> Result<(), LimitExceeded> {
        self.stack.frames.pop();
        if let Some(parent) = self.stack.frames.last()
            && let Ty::List { .. } = self.schema.ty(parent.ty)
        {
            self.queued = Some(Step::Element { list: parent.ty });
        }
        Ok(())
    }

    fn top(&mut self) -> &mut Frame {
        self.stack
            .frames
            .last_mut()
            .expect("the walk has an open value")
    }
}

impl Frame {
    fn unopened(ty: u32, row: u32) -> Self {
        Self {
            ty,
            row,
            next: UNOPENED,
            edges: 0,
            len: 0,
            case: 0,
        }
    }
}

fn expect_kind(row: Option<super::rows::Row>, kind: Kind, ty: u32) -> super::rows::Row {
    let row = row.unwrap_or_else(|| panic!("an unset value fills a required {kind:?} position"));
    assert_eq!(
        row.kind, kind,
        "a {:?} value fills a {kind:?} position of type {ty}",
        row.kind
    );
    row
}
