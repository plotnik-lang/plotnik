use std::collections::HashMap;

use indexmap::IndexSet;

use crate::compiler::analyze::result::ResultSchema;
use crate::compiler::emit::plan::{
    DecodeCase, DecodeItemKind, DecodeScope, DecodeValue, ResultDecodePlan,
};
use crate::compiler::emit::targets::rust::ident::rust_scope_idents;
use crate::compiler::ids::ResultMemberId;
use crate::core::Symbol;

use super::representation::LifetimeUsage;
use super::type_model::{TypeContext, TypeModel};

/// One logical result type as the generated code sees it.
///
/// Positions with the same Rust type share one key, so twin occurrences of
/// a public type and repeated field types map to one schema entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Key {
    Node,
    Text,
    Bool,
    Option(u32),
    List(u32, bool),
    /// A value stored behind a reference, to break by-value recursion.
    Ref(u32),
    /// A named record or variant result item.
    Item(Symbol),
    /// The fields of one variant case, which has no Rust type of its own.
    Payload(Symbol, u32),
}

pub(super) struct FieldModel {
    pub(super) name: String,
    pub(super) ident: String,
    pub(super) ty: u32,
    pub(super) members: Vec<ResultMemberId>,
}

pub(super) struct CaseModel {
    pub(super) name: String,
    pub(super) ident: String,
    pub(super) members: Vec<ResultMemberId>,
    /// The payload record type and its fields; `None` for a tag-only case.
    pub(super) payload: Option<(u32, Vec<FieldModel>)>,
}

pub(super) enum ItemShape {
    Record(Vec<FieldModel>),
    Variant(Vec<CaseModel>),
    Alias(u32),
    MatchOnly,
}

pub(super) struct ItemModel {
    pub(super) name: Symbol,
    pub(super) display: String,
    pub(super) ident: String,
    pub(super) shape: ItemShape,
    /// The schema type of a record or variant item.
    pub(super) ty: Option<u32>,
}

/// The generated Rust representation of a query's results: item types, the
/// logical schema shared with the runtime, and the lifetimes every type uses.
pub(super) struct ResultModel {
    keys: IndexSet<Key>,
    items: Vec<ItemModel>,
    usage: HashMap<Symbol, LifetimeUsage>,
    member_count: usize,
}

impl ResultModel {
    pub(super) fn new(
        schema: &ResultSchema<'_>,
        types: &TypeModel,
        plan: &ResultDecodePlan,
    ) -> Self {
        let mut builder = Builder {
            types,
            schema,
            keys: IndexSet::new(),
            member_count: 0,
        };

        for item in plan.items() {
            if matches!(
                item.kind,
                DecodeItemKind::Record(_) | DecodeItemKind::Variant(_)
            ) {
                builder.intern(Key::Item(item.name));
            }
        }

        let items = plan
            .items()
            .map(|item| {
                let context = item.output.value().map(TypeContext::item);
                let shape = match &item.kind {
                    DecodeItemKind::Record(scope) => {
                        let context = context.expect("a record item has a value type");
                        ItemShape::Record(builder.fields(scope, context))
                    }
                    DecodeItemKind::Variant(cases) => {
                        let context = context.expect("a variant item has a value type");
                        ItemShape::Variant(builder.cases(item.name, cases, context))
                    }
                    DecodeItemKind::Alias(value) => {
                        let context = context.expect("an alias item has a value type");
                        ItemShape::Alias(builder.key(value, context))
                    }
                    DecodeItemKind::MatchOnlyDefinition => ItemShape::MatchOnly,
                };
                let ty = matches!(shape, ItemShape::Record(_) | ItemShape::Variant(_))
                    .then(|| builder.intern(Key::Item(item.name)));
                ItemModel {
                    name: item.name,
                    display: schema.interner.resolve(item.name).to_string(),
                    ident: types.item_ident(item.name).to_string(),
                    shape,
                    ty,
                }
            })
            .collect();

        let mut model = Self {
            keys: builder.keys,
            items,
            usage: HashMap::new(),
            member_count: builder.member_count,
        };
        model.usage = model.compute_usage();
        model
    }

    pub(super) fn items(&self) -> &[ItemModel] {
        &self.items
    }

    pub(super) fn keys(&self) -> impl Iterator<Item = (u32, Key)> + '_ {
        self.keys
            .iter()
            .enumerate()
            .map(|(id, key)| (id as u32, *key))
    }

    pub(super) fn key(&self, id: u32) -> Key {
        self.keys[id as usize]
    }

    pub(super) fn member_count(&self) -> usize {
        self.member_count
    }

    pub(super) fn item(&self, name: Symbol) -> &ItemModel {
        self.items
            .iter()
            .find(|item| item.name == name)
            .expect("every referenced item is modeled")
    }

    pub(super) fn item_usage(&self, name: Symbol) -> LifetimeUsage {
        self.usage[&name]
    }

    /// The Rust type of schema type `id`, naming the storage lifetime `storage`.
    pub(super) fn rust_type(&self, id: u32, storage: &str) -> String {
        match self.key(id) {
            Key::Node => "Node<'t>".to_string(),
            Key::Text => "&'s str".to_string(),
            Key::Bool => "bool".to_string(),
            Key::Option(inner) => {
                format!("::core::option::Option<{}>", self.rust_type(inner, storage))
            }
            Key::List(inner, _) => format!("&{storage} [{}]", self.rust_type(inner, storage)),
            Key::Ref(inner) => format!("&{storage} {}", self.rust_type(inner, storage)),
            Key::Item(name) => {
                let item = self.item(name);
                let params = self.item_usage(name).params(storage);
                format!("{}{params}", item.ident)
            }
            Key::Payload(..) => unreachable!("a case payload has no Rust type"),
        }
    }

    /// Upper bound of a schema type's by-value size, without its target's
    /// real layouts: every value is padded to 8 bytes, and a node handle
    /// takes the runtime's reserved estimate.
    pub(super) fn size_estimate(&self, id: u32) -> u64 {
        const WORD: u64 = 8;
        match self.key(id) {
            Key::Node => plotnik_runtime::GENERATED_NODE_VALUE_BYTES,
            Key::Text | Key::List(..) => 2 * WORD,
            Key::Bool | Key::Ref(_) => WORD,
            Key::Option(inner) => self.size_estimate(inner).saturating_add(WORD),
            Key::Item(name) => match &self.item(name).shape {
                ItemShape::Alias(body) => self.size_estimate(*body),
                ItemShape::Record(fields) => self.fields_estimate(fields),
                ItemShape::Variant(cases) => cases
                    .iter()
                    .map(|case| {
                        case.payload
                            .as_ref()
                            .map_or(0, |(_, fields)| self.fields_estimate(fields))
                    })
                    .max()
                    .unwrap_or(0)
                    .saturating_add(WORD),
                ItemShape::MatchOnly => unreachable!("a match-only item has no value"),
            },
            Key::Payload(..) => unreachable!("a case payload has no Rust type"),
        }
    }

    fn fields_estimate(&self, fields: &[FieldModel]) -> u64 {
        fields
            .iter()
            .map(|field| self.size_estimate(field.ty))
            .fold(0, u64::saturating_add)
    }

    /// Least fixpoint of lifetime use over the item graph. By-value item
    /// cycles cannot exist: recursion always passes through a reference.
    fn compute_usage(&self) -> HashMap<Symbol, LifetimeUsage> {
        let mut usage: HashMap<Symbol, LifetimeUsage> = self
            .items
            .iter()
            .map(|item| (item.name, LifetimeUsage::default()))
            .collect();
        loop {
            let mut changed = false;
            for item in &self.items {
                let fields: Vec<u32> = match &item.shape {
                    ItemShape::Record(fields) => fields.iter().map(|field| field.ty).collect(),
                    ItemShape::Variant(cases) => cases
                        .iter()
                        .filter_map(|case| case.payload.as_ref())
                        .flat_map(|(_, fields)| fields.iter().map(|field| field.ty))
                        .collect(),
                    ItemShape::Alias(ty) => vec![*ty],
                    ItemShape::MatchOnly => Vec::new(),
                };
                let mut next = LifetimeUsage::default();
                for ty in fields {
                    let field = self.usage_with(ty, &usage);
                    next.rows |= field.rows;
                    next.tree |= field.tree;
                    next.source |= field.source;
                }
                if usage[&item.name] != next {
                    usage.insert(item.name, next);
                    changed = true;
                }
            }
            if !changed {
                return usage;
            }
        }
    }

    fn usage_with(&self, id: u32, usage: &HashMap<Symbol, LifetimeUsage>) -> LifetimeUsage {
        match self.key(id) {
            Key::Node => LifetimeUsage {
                tree: true,
                ..LifetimeUsage::default()
            },
            Key::Text => LifetimeUsage {
                source: true,
                ..LifetimeUsage::default()
            },
            Key::Bool | Key::Payload(..) => LifetimeUsage::default(),
            Key::Option(inner) => self.usage_with(inner, usage),
            Key::List(inner, _) | Key::Ref(inner) => LifetimeUsage {
                rows: true,
                ..self.usage_with(inner, usage)
            },
            Key::Item(name) => usage[&name],
        }
    }
}

struct Builder<'b, 'a> {
    types: &'b TypeModel,
    schema: &'b ResultSchema<'a>,
    keys: IndexSet<Key>,
    member_count: usize,
}

impl Builder<'_, '_> {
    fn intern(&mut self, key: Key) -> u32 {
        let (index, _) = self.keys.insert_full(key);
        u32::try_from(index).expect("schema type count fits u32")
    }

    fn note_members(&mut self, members: &[ResultMemberId]) {
        for member in members {
            self.member_count = self.member_count.max(usize::from(member.raw()) + 1);
        }
    }

    fn fields(&mut self, scope: &DecodeScope, context: TypeContext) -> Vec<FieldModel> {
        let names: Vec<&str> = scope
            .fields
            .iter()
            .map(|field| self.schema.interner.resolve(field.name))
            .collect();
        let idents = rust_scope_idents(names.iter().copied());
        scope
            .fields
            .iter()
            .zip(names)
            .zip(idents)
            .map(|((field, name), ident)| {
                self.note_members(&field.indices);
                FieldModel {
                    name: name.to_string(),
                    ident,
                    ty: self.key(&field.value, context),
                    members: field.indices.clone(),
                }
            })
            .collect()
    }

    fn cases(
        &mut self,
        item: Symbol,
        cases: &[DecodeCase],
        context: TypeContext,
    ) -> Vec<CaseModel> {
        let names: Vec<&str> = cases
            .iter()
            .map(|case| self.schema.interner.resolve(case.name))
            .collect();
        let idents = rust_scope_idents(names.iter().copied());
        cases
            .iter()
            .zip(names)
            .zip(idents)
            .enumerate()
            .map(|(index, ((case, name), ident))| {
                self.note_members(&case.indices);
                let payload = case.payload.as_ref().map(|scope| {
                    let ty = self.intern(Key::Payload(item, index as u32));
                    (ty, self.fields(scope, context))
                });
                CaseModel {
                    name: name.to_string(),
                    ident,
                    members: case.indices.clone(),
                    payload,
                }
            })
            .collect()
    }

    fn key(&mut self, value: &DecodeValue, context: TypeContext) -> u32 {
        let key = match value {
            DecodeValue::Node => Key::Node,
            DecodeValue::Text => Key::Text,
            DecodeValue::Bool => Key::Bool,
            DecodeValue::Option(inner) => Key::Option(self.key(inner, context)),
            DecodeValue::List { element, minimum } => {
                Key::List(self.key(element, context.list_element()), *minimum)
            }
            DecodeValue::Nested { item, source_type } => {
                let target = self.intern(Key::Item(*item));
                if !self.types.is_boxed_ref(context, *source_type) {
                    return target;
                }
                Key::Ref(target)
            }
        };
        self.intern(key)
    }
}
