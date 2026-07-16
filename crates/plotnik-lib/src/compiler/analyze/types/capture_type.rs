//! Frozen semantics for capture types.
//!
//! Inference first validates the ordinary capture and records its
//! [`CaptureKind`]. A built-in capture type then contributes one immutable plan
//! describing how lowering maps that already-valid value. Lowering executes the
//! plan; it never interprets the spelling after `::` or reclassifies the raw
//! capture.

use std::collections::BTreeMap;

use crate::compiler::ids::TypeId;
use crate::core::Symbol;

use super::CaptureKind;
use super::type_shape::RecordField;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltInCaptureType {
    Text,
    Bool,
}

impl BuiltInCaptureType {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "text" => Some(Self::Text),
            "bool" => Some(Self::Bool),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerminalData {
    NodeRepresentation,
    Semantic,
}

impl TerminalData {
    pub fn suppresses_semantic_data(self) -> bool {
        self == Self::Semantic
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OptionMode {
    Preserve,
    Bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CaptureTypePlanKind {
    TextTerminal {
        data: TerminalData,
    },
    BoolTerminal {
        data: TerminalData,
    },
    Option {
        mode: OptionMode,
        inner: Box<CaptureTypePlan>,
    },
    List {
        element: Box<CaptureTypePlan>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CaptureTypePlan {
    final_type: TypeId,
    kind: CaptureTypePlanKind,
}

impl CaptureTypePlan {
    pub fn text_terminal(final_type: TypeId, data: TerminalData) -> Self {
        Self {
            final_type,
            kind: CaptureTypePlanKind::TextTerminal { data },
        }
    }

    pub fn bool_terminal(final_type: TypeId, data: TerminalData) -> Self {
        Self {
            final_type,
            kind: CaptureTypePlanKind::BoolTerminal { data },
        }
    }

    pub fn option(final_type: TypeId, mode: OptionMode, inner: CaptureTypePlan) -> Self {
        Self {
            final_type,
            kind: CaptureTypePlanKind::Option {
                mode,
                inner: Box::new(inner),
            },
        }
    }

    pub fn list(final_type: TypeId, element: CaptureTypePlan) -> Self {
        Self {
            final_type,
            kind: CaptureTypePlanKind::List {
                element: Box::new(element),
            },
        }
    }

    pub fn final_type(&self) -> TypeId {
        self.final_type
    }

    pub fn kind(&self) -> &CaptureTypePlanKind {
        &self.kind
    }

    pub fn suppresses_semantic_data(&self) -> bool {
        match &self.kind {
            CaptureTypePlanKind::TextTerminal { data }
            | CaptureTypePlanKind::BoolTerminal { data } => data.suppresses_semantic_data(),
            CaptureTypePlanKind::Option { inner, .. } => inner.suppresses_semantic_data(),
            CaptureTypePlanKind::List { element } => element.suppresses_semantic_data(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawCaptureFact {
    kind: CaptureKind,
    field: RecordField,
    valid: bool,
}

impl RawCaptureFact {
    pub fn admitted(kind: CaptureKind, field: RecordField) -> Self {
        Self {
            kind,
            field,
            valid: true,
        }
    }

    pub fn rejected(kind: CaptureKind, field: RecordField) -> Self {
        Self {
            kind,
            field,
            valid: false,
        }
    }

    pub fn kind(&self) -> CaptureKind {
        self.kind
    }

    pub fn field(&self) -> RecordField {
        self.field
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }
}

#[derive(Clone, Debug)]
pub struct CaptureFact {
    kind: CaptureKind,
    behavior: CaptureBehavior,
}

#[derive(Clone, Debug)]
enum CaptureBehavior {
    Ordinary,
    BuiltIn {
        capture_type: BuiltInCaptureType,
        plan: CaptureTypePlan,
    },
}

impl CaptureFact {
    pub fn ordinary(kind: CaptureKind) -> Self {
        Self {
            kind,
            behavior: CaptureBehavior::Ordinary,
        }
    }

    pub fn built_in(
        kind: CaptureKind,
        capture_type: BuiltInCaptureType,
        plan: CaptureTypePlan,
    ) -> Self {
        Self {
            kind,
            behavior: CaptureBehavior::BuiltIn { capture_type, plan },
        }
    }

    pub fn kind(&self) -> CaptureKind {
        self.kind
    }

    pub fn built_in_plan(&self) -> Option<(BuiltInCaptureType, &CaptureTypePlan)> {
        match &self.behavior {
            CaptureBehavior::Ordinary => None,
            CaptureBehavior::BuiltIn { capture_type, plan } => Some((*capture_type, plan)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldCompletion {
    /// Every alternative produces the field, so lowering owes no value.
    AlwaysPresent,
    /// A non-producing alternative materializes semantic absence (`null` in JSON).
    Absent,
    /// A non-producing alternative materializes an empty list.
    EmptyList,
    /// A non-producing alternative materializes a false presence value.
    False,
}

/// Total completion behavior for the fields merged by one alternation.
#[derive(Clone, Debug, Default)]
pub struct FieldCompletions {
    by_field: BTreeMap<Symbol, FieldCompletion>,
}

impl FieldCompletions {
    pub fn new(by_field: BTreeMap<Symbol, FieldCompletion>) -> Self {
        Self { by_field }
    }

    pub fn completion(&self, field: Symbol) -> FieldCompletion {
        self.by_field
            .get(&field)
            .copied()
            .expect("every merged field must have an explicit completion")
    }

    pub(crate) fn fields(&self) -> impl Iterator<Item = Symbol> + '_ {
        self.by_field.keys().copied()
    }
}
