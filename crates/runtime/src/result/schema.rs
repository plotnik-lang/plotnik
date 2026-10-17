use std::ops::Range;

use crate::Buffer;

/// Owned dynamic tables. Names borrow the validated query module, not the
/// reusable VM or its result tape. Documents share these admitted capacities.
pub struct SchemaTables<'q> {
    pub types: Buffer<Ty>,
    pub fields: Buffer<Field<'q>>,
    pub cases: Buffer<Case<'q>>,
    pub members: Buffer<Member>,
}

impl<'q> SchemaTables<'q> {
    pub fn new() -> Self {
        Self {
            types: Buffer::new(),
            fields: Buffer::new(),
            cases: Buffer::new(),
            members: Buffer::new(),
        }
    }

    pub fn view(&self) -> Schema<'_> {
        Schema {
            types: self.types.as_slice(),
            fields: self.fields.as_slice(),
            cases: self.cases.as_slice(),
            members: self.members.as_slice(),
        }
    }
}

impl Default for SchemaTables<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// A contiguous range of one of a [`Schema`]'s tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub len: u32,
}

impl Span {
    pub const fn new(start: u32, len: u32) -> Self {
        Self { start, len }
    }

    pub(crate) fn range(self) -> Range<usize> {
        let start = self.start as usize;
        start..start + self.len as usize
    }
}

/// One logical result type. Type operands index the same schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    /// A match-only entry has no result payload.
    NoValue,
    Node,
    Text,
    Bool,
    /// `None` unless a value is present.
    Option(u32),
    /// Zero or more elements. A field never set holds an empty list.
    List {
        element: u32,
        minimum: u32,
    },
    /// The value is stored behind a reference into the result block. Only
    /// generated publishers use it, to break recursive by-value types.
    Ref(u32),
    /// Named fields, in canonical order.
    Record {
        fields: Span,
    },
    /// Exactly one case.
    Variant {
        cases: Span,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field<'a> {
    pub name: &'a str,
    pub ty: u32,
}

/// One variant case. `payload` is a record type, or [`Case::NO_PAYLOAD`] for
/// a tag-only case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case<'a> {
    pub name: &'a str,
    pub payload: u32,
}

impl Case<'_> {
    pub const NO_PAYLOAD: u32 = u32::MAX;

    pub fn payload(&self) -> Option<u32> {
        (self.payload != Self::NO_PAYLOAD).then_some(self.payload)
    }
}

/// What the member operand of `RecordSet` or `VariantOpen` selects: field
/// `index` of record `owner`, or case `index` of variant `owner`.
///
/// Twin occurrences of one public type map several members to the same entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    pub owner: u32,
    pub index: u32,
}

impl Member {
    /// A member that no accepted output can select.
    pub const UNUSED: Self = Self {
        owner: u32::MAX,
        index: u32::MAX,
    };

    pub const fn new(owner: u32, index: u32) -> Self {
        Self { owner, index }
    }
}

/// The logical result schema shared by the VM and generated publishers.
///
/// Normalization, validation, defaults (an unset option is `None`, an unset
/// list is empty) and JSON rendering all read this one description. Generated
/// modules embed it as static data; the VM derives it from bytecode.
#[derive(Clone, Copy, Debug)]
pub struct Schema<'a> {
    pub types: &'a [Ty],
    pub fields: &'a [Field<'a>],
    pub cases: &'a [Case<'a>],
    pub members: &'a [Member],
}

impl<'a> Schema<'a> {
    pub(crate) fn ty(&self, id: u32) -> Ty {
        *self
            .types
            .get(id as usize)
            .unwrap_or_else(|| panic!("result type {id} is outside the schema"))
    }

    pub(crate) fn member(&self, id: u32) -> Member {
        let member = *self
            .members
            .get(id as usize)
            .unwrap_or_else(|| panic!("output member {id} is outside the schema"));
        assert!(
            member != Member::UNUSED,
            "output member {id} belongs to no result type"
        );
        member
    }

    /// The fields of a record type.
    pub(crate) fn fields(&self, record: u32) -> &'a [Field<'a>] {
        match self.ty(record) {
            Ty::Record { fields } => &self.fields[fields.range()],
            other => panic!("result type {record} is not a record: {other:?}"),
        }
    }

    pub(crate) fn case(&self, variant: u32, index: u32) -> Case<'a> {
        let Ty::Variant { cases } = self.ty(variant) else {
            panic!("result type {variant} is not a variant");
        };
        assert!(index < cases.len, "variant {variant} has no case {index}");
        self.cases[cases.start as usize + index as usize]
    }

    /// The fields set between `VariantOpen` and `VariantClose` of one case.
    pub(crate) fn case_fields(&self, variant: u32, index: u32) -> &'a [Field<'a>] {
        match self.case(variant, index).payload() {
            Some(payload) => self.fields(payload),
            None => &[],
        }
    }
}
