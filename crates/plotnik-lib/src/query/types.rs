//! Type inference pass: AST → TypeTable.
//!
//! Walks definitions and infers output types from capture patterns.
//! Produces a `TypeTable` containing all inferred types.

use indexmap::{IndexMap, IndexSet};
use rowan::TextRange;

use crate::diagnostics::DiagnosticKind;
use crate::infer::{MergedField, TypeKey, TypeTable, TypeValue};
use crate::parser::{AltKind, Expr, ast, token_src};

use super::Query;

/// Tracks a field's type and the location where it was first captured.
#[derive(Clone)]
struct FieldEntry<'src> {
    type_key: TypeKey<'src>,
    /// Range of the capture name token (e.g., `@x`)
    capture_range: TextRange,
}

impl<'a> Query<'a> {
    pub(super) fn infer_types(&mut self) {
        let mut ctx = InferContext::new(self.source);

        let defs: Vec<_> = self.ast.defs().collect();
        let last_idx = defs.len().saturating_sub(1);

        for (idx, def) in defs.iter().enumerate() {
            let is_last = idx == last_idx;
            ctx.infer_def(def, is_last);
        }

        ctx.mark_cyclic_types();

        self.type_table = ctx.table;
        self.type_diagnostics = ctx.diagnostics;
    }
}

struct InferContext<'src> {
    source: &'src str,
    table: TypeTable<'src>,
    diagnostics: crate::diagnostics::Diagnostics,
    /// Counter for generating unique synthetic type keys
    synthetic_counter: usize,
}

impl<'src> InferContext<'src> {
    fn new(source: &'src str) -> Self {
        Self {
            source,
            table: TypeTable::new(),
            diagnostics: crate::diagnostics::Diagnostics::new(),
            synthetic_counter: 0,
        }
    }

    /// Generate a unique suffix for synthetic keys
    fn next_synthetic_suffix(&mut self) -> &'src str {
        let n = self.synthetic_counter;
        self.synthetic_counter += 1;
        // Leak a small string for the lifetime - this is fine for query processing
        Box::leak(n.to_string().into_boxed_str())
    }

    /// Mark types that contain cyclic references (need Box/Rc/Arc in Rust).
    /// Only struct/union types are marked - wrapper types (Optional, List, etc.)
    /// shouldn't be wrapped in Box themselves, only their inner references.
    fn mark_cyclic_types(&mut self) {
        let keys: Vec<_> = self
            .table
            .types
            .keys()
            .filter(|k| !k.is_builtin())
            .filter(|k| {
                matches!(
                    self.table.get(k),
                    Some(TypeValue::Struct(_)) | Some(TypeValue::TaggedUnion(_))
                )
            })
            .cloned()
            .collect();

        for key in keys {
            if self.type_references_itself(&key) {
                self.table.mark_cyclic(key);
            }
        }
    }

    /// Check if a type contains a reference to itself (directly or indirectly).
    fn type_references_itself(&self, key: &TypeKey<'src>) -> bool {
        let mut visited = IndexSet::new();
        self.type_reaches(key, key, &mut visited)
    }

    /// Check if `current` type can reach `target` type through references.
    fn type_reaches(
        &self,
        current: &TypeKey<'src>,
        target: &TypeKey<'src>,
        visited: &mut IndexSet<TypeKey<'src>>,
    ) -> bool {
        if !visited.insert(current.clone()) {
            return false;
        }

        let Some(value) = self.table.get(current) else {
            return false;
        };

        match value {
            TypeValue::Struct(fields) => {
                for field_key in fields.values() {
                    if field_key == target {
                        return true;
                    }
                    if self.type_reaches(field_key, target, visited) {
                        return true;
                    }
                }
                false
            }
            TypeValue::TaggedUnion(variants) => {
                for variant_key in variants.values() {
                    if variant_key == target {
                        return true;
                    }
                    if self.type_reaches(variant_key, target, visited) {
                        return true;
                    }
                }
                false
            }
            TypeValue::Optional(inner)
            | TypeValue::List(inner)
            | TypeValue::NonEmptyList(inner) => {
                if inner == target {
                    return true;
                }
                self.type_reaches(inner, target, visited)
            }
            TypeValue::Node | TypeValue::String | TypeValue::Unit | TypeValue::Invalid => false,
        }
    }

    fn infer_def(&mut self, def: &ast::Def, is_last: bool) {
        let key = match def.name() {
            Some(name_tok) => {
                let name = token_src(&name_tok, self.source);
                TypeKey::Named(name)
            }
            None if is_last => TypeKey::DefaultQuery,
            None => return, // unnamed non-last def, already reported by earlier pass
        };

        let Some(body) = def.body() else {
            return;
        };

        // Special case: tagged alternation at def level produces TaggedUnion directly
        if let Expr::AltExpr(alt) = &body
            && matches!(alt.kind(), AltKind::Tagged)
        {
            let type_annotation = match &key {
                TypeKey::Named(name) => Some(*name),
                _ => None,
            };
            self.infer_tagged_alt(alt, &key, type_annotation);
            return;
        }

        let mut fields = IndexMap::new();
        self.infer_expr(&body, &key, &mut fields);

        let value = if fields.is_empty() {
            TypeValue::Unit
        } else {
            TypeValue::Struct(Self::extract_types(fields))
        };

        self.table.insert(key, value);
    }

    /// Extract just the types from field entries
    fn extract_types(
        fields: IndexMap<&'src str, FieldEntry<'src>>,
    ) -> IndexMap<&'src str, TypeKey<'src>> {
        fields.into_iter().map(|(k, v)| (k, v.type_key)).collect()
    }

    /// Extract types by reference for merge operations
    fn extract_types_ref(
        fields: &IndexMap<&'src str, FieldEntry<'src>>,
    ) -> IndexMap<&'src str, TypeKey<'src>> {
        fields
            .iter()
            .map(|(k, v)| (*k, v.type_key.clone()))
            .collect()
    }

    /// Infer type for an expression, collecting captures into `fields`.
    /// Returns the TypeKey if this expression produces a referenceable type.
    fn infer_expr(
        &mut self,
        expr: &Expr,
        parent: &TypeKey<'src>,
        fields: &mut IndexMap<&'src str, FieldEntry<'src>>,
    ) -> Option<TypeKey<'src>> {
        match expr {
            Expr::NamedNode(node) => {
                for child in node.children() {
                    self.infer_expr(&child, parent, fields);
                }
                Some(TypeKey::Node)
            }

            Expr::AnonymousNode(_) => Some(TypeKey::Node),

            Expr::Ref(r) => {
                let name_tok = r.name()?;
                let name = token_src(&name_tok, self.source);
                Some(TypeKey::Named(name))
            }

            Expr::SeqExpr(seq) => {
                for child in seq.children() {
                    self.infer_expr(&child, parent, fields);
                }
                None
            }

            Expr::FieldExpr(field) => {
                if let Some(value) = field.value() {
                    self.infer_expr(&value, parent, fields);
                }
                None
            }

            Expr::CapturedExpr(cap) => self.infer_capture(cap, parent, fields),

            Expr::QuantifiedExpr(quant) => self.infer_quantified(quant, parent, fields),

            Expr::AltExpr(alt) => self.infer_alt(alt, parent, fields),
        }
    }

    fn infer_capture(
        &mut self,
        cap: &ast::CapturedExpr,
        parent: &TypeKey<'src>,
        fields: &mut IndexMap<&'src str, FieldEntry<'src>>,
    ) -> Option<TypeKey<'src>> {
        let name_tok = cap.name()?;
        let capture_name = token_src(&name_tok, self.source);
        let capture_range = name_tok.text_range();

        let type_annotation = cap.type_annotation().and_then(|t| {
            let tok = t.name()?;
            Some(token_src(&tok, self.source))
        });

        let inner = cap.inner();

        // Flat extraction: collect nested captures from inner expression into outer fields
        // Only for NamedNode/AnonymousNode - Seq/Alt create their own scopes when captured
        if let Some(ref inner_expr) = inner {
            match inner_expr {
                Expr::NamedNode(node) => {
                    for child in node.children() {
                        self.infer_expr(&child, parent, fields);
                    }
                }
                Expr::FieldExpr(field) => {
                    if let Some(value) = field.value() {
                        self.infer_expr(&value, parent, fields);
                    }
                }
                _ => {}
            }
        }

        let inner_type =
            self.infer_capture_inner(inner.as_ref(), parent, capture_name, type_annotation);

        // Check for duplicate capture in scope
        // Unlike alternations (where branches are mutually exclusive),
        // in sequences both captures execute - can't have two values for same name
        if let Some(existing) = fields.get(capture_name) {
            self.diagnostics
                .report(DiagnosticKind::DuplicateCaptureInScope, capture_range)
                .message(capture_name)
                .related_to("first use", existing.capture_range)
                .emit();
            fields.insert(
                capture_name,
                FieldEntry {
                    type_key: TypeKey::Invalid,
                    capture_range,
                },
            );
            return Some(TypeKey::Invalid);
        }

        fields.insert(
            capture_name,
            FieldEntry {
                type_key: inner_type.clone(),
                capture_range,
            },
        );
        Some(inner_type)
    }

    fn infer_capture_inner(
        &mut self,
        inner: Option<&Expr>,
        parent: &TypeKey<'src>,
        capture_name: &'src str,
        type_annotation: Option<&'src str>,
    ) -> TypeKey<'src> {
        // Handle quantifier first - it wraps whatever the inner type is
        // This ensures `(x)+ @name :: string` becomes Vec<String>, not String
        if let Some(Expr::QuantifiedExpr(q)) = inner {
            let Some(qinner) = q.inner() else {
                return TypeKey::Invalid;
            };
            let inner_key =
                self.infer_capture_inner(Some(&qinner), parent, capture_name, type_annotation);
            return self.wrap_with_quantifier(&inner_key, q, parent, capture_name);
        }

        // :: string annotation
        if type_annotation == Some("string") {
            return TypeKey::String;
        }

        let Some(inner) = inner else {
            return type_annotation.map(TypeKey::Named).unwrap_or(TypeKey::Node);
        };

        match inner {
            Expr::Ref(r) => {
                if let Some(name_tok) = r.name() {
                    let ref_name = token_src(&name_tok, self.source);
                    TypeKey::Named(ref_name)
                } else {
                    TypeKey::Invalid
                }
            }

            Expr::SeqExpr(_) => {
                self.infer_nested_scope(inner, parent, capture_name, type_annotation, || {
                    inner.children().into_iter().collect()
                })
            }

            Expr::AltExpr(alt) => {
                self.infer_nested_scope(inner, parent, capture_name, type_annotation, || {
                    alt.branches().filter_map(|b| b.body()).collect()
                })
            }

            Expr::NamedNode(_) | Expr::AnonymousNode(_) => {
                type_annotation.map(TypeKey::Named).unwrap_or(TypeKey::Node)
            }

            Expr::QuantifiedExpr(_) => {
                unreachable!("quantifier handled at start of function")
            }

            Expr::FieldExpr(field) => {
                if let Some(value) = field.value() {
                    self.infer_capture_inner(Some(&value), parent, capture_name, type_annotation)
                } else {
                    type_annotation.map(TypeKey::Named).unwrap_or(TypeKey::Node)
                }
            }

            Expr::CapturedExpr(_) => type_annotation.map(TypeKey::Named).unwrap_or(TypeKey::Node),
        }
    }

    fn infer_nested_scope<F>(
        &mut self,
        inner: &Expr,
        parent: &TypeKey<'src>,
        capture_name: &'src str,
        type_annotation: Option<&'src str>,
        get_children: F,
    ) -> TypeKey<'src>
    where
        F: FnOnce() -> Vec<Expr>,
    {
        let nested_parent = TypeKey::Synthetic {
            parent: Box::new(parent.clone()),
            path: vec![capture_name],
        };

        let mut nested_fields = IndexMap::new();

        match inner {
            Expr::AltExpr(alt) => {
                let alt_key = self.infer_alt_as_type(alt, &nested_parent, type_annotation);
                return alt_key;
            }
            _ => {
                for child in get_children() {
                    self.infer_expr(&child, &nested_parent, &mut nested_fields);
                }
            }
        }

        if nested_fields.is_empty() {
            return type_annotation.map(TypeKey::Named).unwrap_or(TypeKey::Node);
        }

        let key = if let Some(name) = type_annotation {
            TypeKey::Named(name)
        } else {
            // Use unique suffix to allow same capture name in different alternation branches
            let suffix = self.next_synthetic_suffix();
            TypeKey::Synthetic {
                parent: Box::new(parent.clone()),
                path: vec![capture_name, suffix],
            }
        };

        self.table.insert(
            key.clone(),
            TypeValue::Struct(Self::extract_types(nested_fields)),
        );
        key
    }

    fn infer_quantified(
        &mut self,
        quant: &ast::QuantifiedExpr,
        parent: &TypeKey<'src>,
        fields: &mut IndexMap<&'src str, FieldEntry<'src>>,
    ) -> Option<TypeKey<'src>> {
        let inner = quant.inner()?;
        quant.operator()?;

        // If the inner is a capture, we need special handling for the wrapper
        if let Expr::CapturedExpr(cap) = &inner {
            let name_tok = cap.name()?;
            let capture_name = token_src(&name_tok, self.source);
            let capture_range = name_tok.text_range();

            let type_annotation = cap.type_annotation().and_then(|t| {
                let tok = t.name()?;
                Some(token_src(&tok, self.source))
            });

            let inner_key = self.infer_capture_inner(
                cap.inner().as_ref(),
                parent,
                capture_name,
                type_annotation,
            );
            let wrapped_key = self.wrap_with_quantifier(&inner_key, quant, parent, capture_name);

            fields.insert(
                capture_name,
                FieldEntry {
                    type_key: wrapped_key.clone(),
                    capture_range,
                },
            );
            return Some(wrapped_key);
        }

        // Non-capture quantified expression: track fields added by inner expression
        // and wrap them with the quantifier
        let fields_before: Vec<_> = fields.keys().copied().collect();

        let inner_key = self.infer_expr(&inner, parent, fields)?;

        // Wrap all newly added fields with the quantifier
        let field_names: Vec<_> = fields.keys().copied().collect();
        for name in field_names {
            if fields_before.contains(&name) {
                continue;
            }
            if let Some(entry) = fields.get_mut(name) {
                entry.type_key = self.wrap_with_quantifier(&entry.type_key, quant, parent, name);
            }
        }

        // Return wrapped inner key (though typically unused when wrapping field captures)
        Some(inner_key)
    }

    fn wrap_with_quantifier(
        &mut self,
        inner: &TypeKey<'src>,
        quant: &ast::QuantifiedExpr,
        parent: &TypeKey<'src>,
        capture_name: &'src str,
    ) -> TypeKey<'src> {
        if matches!(inner, TypeKey::Invalid) {
            return TypeKey::Invalid;
        }

        // Check list/non-empty-list before optional since * matches both is_list() and is_optional()
        let wrapper = if quant.is_list() {
            TypeValue::List(inner.clone())
        } else if quant.is_non_empty_list() {
            TypeValue::NonEmptyList(inner.clone())
        } else if quant.is_optional() {
            TypeValue::Optional(inner.clone())
        } else {
            return inner.clone();
        };

        // Synthetic key: Parent + capture_name → e.g., QueryResultItems
        let wrapper_key = TypeKey::Synthetic {
            parent: Box::new(parent.clone()),
            path: vec![capture_name],
        };

        self.table.insert(wrapper_key.clone(), wrapper);
        wrapper_key
    }

    fn infer_alt(
        &mut self,
        alt: &ast::AltExpr,
        parent: &TypeKey<'src>,
        fields: &mut IndexMap<&'src str, FieldEntry<'src>>,
    ) -> Option<TypeKey<'src>> {
        // Alt without capture: just collect fields from all branches into current scope
        match alt.kind() {
            AltKind::Tagged => {
                // Tagged alt without capture: unusual, but collect fields
                for branch in alt.branches() {
                    if let Some(body) = branch.body() {
                        self.infer_expr(&body, parent, fields);
                    }
                }
            }
            AltKind::Untagged | AltKind::Mixed => {
                // Untagged alt: merge fields from branches
                let branch_fields = self.collect_branch_fields(alt, parent);
                let branch_types: Vec<_> =
                    branch_fields.iter().map(Self::extract_types_ref).collect();
                let merged = self.table.merge_fields(&branch_types);
                self.apply_merged_fields(merged, fields, alt, parent);
            }
        }
        None
    }

    fn infer_alt_as_type(
        &mut self,
        alt: &ast::AltExpr,
        parent: &TypeKey<'src>,
        type_annotation: Option<&'src str>,
    ) -> TypeKey<'src> {
        match alt.kind() {
            AltKind::Tagged => self.infer_tagged_alt(alt, parent, type_annotation),
            AltKind::Untagged | AltKind::Mixed => {
                self.infer_untagged_alt(alt, parent, type_annotation)
            }
        }
    }

    fn infer_tagged_alt(
        &mut self,
        alt: &ast::AltExpr,
        parent: &TypeKey<'src>,
        type_annotation: Option<&'src str>,
    ) -> TypeKey<'src> {
        let mut variants = IndexMap::new();

        for branch in alt.branches() {
            let Some(label_tok) = branch.label() else {
                continue;
            };
            let label = token_src(&label_tok, self.source);

            let variant_key = TypeKey::Synthetic {
                parent: Box::new(parent.clone()),
                path: vec![label],
            };

            let mut variant_fields = IndexMap::new();
            let body_type = if let Some(body) = branch.body() {
                self.infer_expr(&body, &variant_key, &mut variant_fields)
            } else {
                None
            };

            let variant_value = if variant_fields.is_empty() {
                // No captures: check if the body produced a meaningful type
                match body_type {
                    Some(key) if !key.is_builtin() => {
                        // Branch body has a non-builtin type (e.g., Ref or wrapped type)
                        // Create a struct with a "value" field
                        let mut fields = IndexMap::new();
                        fields.insert("value", key);
                        TypeValue::Struct(fields)
                    }
                    _ => TypeValue::Unit,
                }
            } else {
                TypeValue::Struct(Self::extract_types(variant_fields))
            };

            // Variant types shouldn't conflict - they have unique paths including the label
            self.table.insert(variant_key.clone(), variant_value);
            variants.insert(label, variant_key);
        }

        let union_key = if let Some(name) = type_annotation {
            TypeKey::Named(name)
        } else {
            parent.clone()
        };

        // Detect conflict: same key with incompatible TaggedUnion
        let current_span = alt.text_range();
        if let Err(existing_span) = self.table.try_insert(
            union_key.clone(),
            TypeValue::TaggedUnion(variants),
            current_span,
        ) {
            self.diagnostics
                .report(
                    DiagnosticKind::IncompatibleTaggedAlternations,
                    existing_span,
                )
                .related_to("incompatible", current_span)
                .emit();
            return union_key;
        }
        union_key
    }

    fn infer_untagged_alt(
        &mut self,
        alt: &ast::AltExpr,
        parent: &TypeKey<'src>,
        type_annotation: Option<&'src str>,
    ) -> TypeKey<'src> {
        let branch_fields = self.collect_branch_fields(alt, parent);
        let branch_types: Vec<_> = branch_fields.iter().map(Self::extract_types_ref).collect();
        let merged = self.table.merge_fields(&branch_types);

        if merged.is_empty() {
            return type_annotation.map(TypeKey::Named).unwrap_or(TypeKey::Node);
        }

        let mut result_fields = IndexMap::new();
        self.apply_merged_fields(merged, &mut result_fields, alt, parent);

        let key = if let Some(name) = type_annotation {
            TypeKey::Named(name)
        } else {
            // Use unique suffix to allow same capture name in different alternation branches
            let suffix = self.next_synthetic_suffix();
            TypeKey::Synthetic {
                parent: Box::new(parent.clone()),
                path: vec![suffix],
            }
        };

        self.table.insert(
            key.clone(),
            TypeValue::Struct(Self::extract_types(result_fields)),
        );
        key
    }

    fn collect_branch_fields(
        &mut self,
        alt: &ast::AltExpr,
        parent: &TypeKey<'src>,
    ) -> Vec<IndexMap<&'src str, FieldEntry<'src>>> {
        let mut branch_fields = Vec::new();

        for branch in alt.branches() {
            let mut fields = IndexMap::new();
            if let Some(body) = branch.body() {
                self.infer_expr(&body, parent, &mut fields);
            }
            branch_fields.push(fields);
        }

        branch_fields
    }

    fn apply_merged_fields(
        &mut self,
        merged: IndexMap<&'src str, MergedField<'src>>,
        result_fields: &mut IndexMap<&'src str, FieldEntry<'src>>,
        alt: &ast::AltExpr,
        parent: &TypeKey<'src>,
    ) {
        for (name, merge_result) in merged {
            let key = match merge_result {
                MergedField::Same(k) => k,
                MergedField::Optional(k) => {
                    let wrapper_key = TypeKey::Synthetic {
                        parent: Box::new(parent.clone()),
                        path: vec![name, "opt"],
                    };
                    self.table
                        .insert(wrapper_key.clone(), TypeValue::Optional(k));
                    wrapper_key
                }
                MergedField::Conflict => {
                    self.diagnostics
                        .report(DiagnosticKind::TypeConflictInMerge, alt.text_range())
                        .message(name)
                        .emit();
                    TypeKey::Invalid
                }
            };
            result_fields.insert(
                name,
                FieldEntry {
                    type_key: key,
                    // Use the alt's range as a fallback since we don't have individual capture ranges here
                    capture_range: alt.text_range(),
                },
            );
        }
    }
}
