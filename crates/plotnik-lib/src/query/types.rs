//! Type inference pass: AST → TypeTable.
//!
//! Walks definitions and infers output types from capture patterns.
//! Produces a `TypeTable` containing all inferred types.

use indexmap::IndexMap;

use crate::diagnostics::DiagnosticKind;
use crate::infer::{MergedField, TypeKey, TypeTable, TypeValue};
use crate::parser::cst::SyntaxKind;
use crate::parser::{AltKind, Expr, ast, token_src};

use super::Query;

impl<'a> Query<'a> {
    pub(super) fn infer_types(&mut self) {
        let mut ctx = InferContext::new(self.source);

        let defs: Vec<_> = self.ast.defs().collect();
        let last_idx = defs.len().saturating_sub(1);

        for (idx, def) in defs.iter().enumerate() {
            let is_last = idx == last_idx;
            ctx.infer_def(def, is_last);
        }

        self.type_table = ctx.table;
        self.type_diagnostics = ctx.diagnostics;
    }
}

struct InferContext<'src> {
    source: &'src str,
    table: TypeTable<'src>,
    diagnostics: crate::diagnostics::Diagnostics,
}

impl<'src> InferContext<'src> {
    fn new(source: &'src str) -> Self {
        Self {
            source,
            table: TypeTable::new(),
            diagnostics: crate::diagnostics::Diagnostics::new(),
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

        let path = match &key {
            TypeKey::Named(name) => vec![*name],
            TypeKey::DefaultQuery => vec![],
            _ => vec![],
        };

        // Special case: tagged alternation at def level produces TaggedUnion directly
        if let Expr::AltExpr(alt) = &body
            && matches!(alt.kind(), AltKind::Tagged)
        {
            let type_annotation = match &key {
                TypeKey::Named(name) => Some(*name),
                _ => None,
            };
            self.infer_tagged_alt(alt, &path, type_annotation);
            return;
        }

        let mut fields = IndexMap::new();
        self.infer_expr(&body, &path, &mut fields);

        let value = if fields.is_empty() {
            TypeValue::Unit
        } else {
            TypeValue::Struct(fields)
        };

        self.table.insert(key, value);
    }

    /// Infer type for an expression, collecting captures into `fields`.
    /// Returns the TypeKey if this expression produces a referenceable type.
    fn infer_expr(
        &mut self,
        expr: &Expr,
        path: &[&'src str],
        fields: &mut IndexMap<&'src str, TypeKey<'src>>,
    ) -> Option<TypeKey<'src>> {
        match expr {
            Expr::NamedNode(node) => {
                for child in node.children() {
                    self.infer_expr(&child, path, fields);
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
                    self.infer_expr(&child, path, fields);
                }
                None
            }

            Expr::FieldExpr(field) => {
                if let Some(value) = field.value() {
                    self.infer_expr(&value, path, fields);
                }
                None
            }

            Expr::CapturedExpr(cap) => self.infer_capture(cap, path, fields),

            Expr::QuantifiedExpr(quant) => self.infer_quantified(quant, path, fields),

            Expr::AltExpr(alt) => self.infer_alt(alt, path, fields),
        }
    }

    fn infer_capture(
        &mut self,
        cap: &ast::CapturedExpr,
        path: &[&'src str],
        fields: &mut IndexMap<&'src str, TypeKey<'src>>,
    ) -> Option<TypeKey<'src>> {
        let name_tok = cap.name()?;
        let capture_name = token_src(&name_tok, self.source);

        let type_annotation = cap.type_annotation().and_then(|t| {
            let tok = t.name()?;
            Some(token_src(&tok, self.source))
        });

        let inner = cap.inner();
        let inner_type =
            self.infer_capture_inner(inner.as_ref(), path, capture_name, type_annotation);

        fields.insert(capture_name, inner_type.clone());
        Some(inner_type)
    }

    fn infer_capture_inner(
        &mut self,
        inner: Option<&Expr>,
        path: &[&'src str],
        capture_name: &'src str,
        type_annotation: Option<&'src str>,
    ) -> TypeKey<'src> {
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

            Expr::SeqExpr(seq) => {
                self.infer_nested_scope(inner, path, capture_name, type_annotation, || {
                    seq.children().collect()
                })
            }

            Expr::AltExpr(alt) => {
                self.infer_nested_scope(inner, path, capture_name, type_annotation, || {
                    alt.branches().filter_map(|b| b.body()).collect()
                })
            }

            Expr::NamedNode(_) | Expr::AnonymousNode(_) => {
                type_annotation.map(TypeKey::Named).unwrap_or(TypeKey::Node)
            }

            Expr::QuantifiedExpr(q) => {
                if let Some(qinner) = q.inner() {
                    let inner_key = self.infer_capture_inner(
                        Some(&qinner),
                        path,
                        capture_name,
                        type_annotation,
                    );
                    if let Some(op) = q.operator() {
                        self.wrap_with_quantifier(&inner_key, op.kind())
                    } else {
                        inner_key
                    }
                } else {
                    TypeKey::Invalid
                }
            }

            Expr::CapturedExpr(_) | Expr::FieldExpr(_) => {
                type_annotation.map(TypeKey::Named).unwrap_or(TypeKey::Node)
            }
        }
    }

    fn infer_nested_scope<F>(
        &mut self,
        inner: &Expr,
        path: &[&'src str],
        capture_name: &'src str,
        type_annotation: Option<&'src str>,
        get_children: F,
    ) -> TypeKey<'src>
    where
        F: FnOnce() -> Vec<Expr>,
    {
        let mut nested_path = path.to_vec();
        nested_path.push(capture_name);

        let mut nested_fields = IndexMap::new();

        match inner {
            Expr::AltExpr(alt) => {
                let alt_key = self.infer_alt_as_type(alt, &nested_path, type_annotation);
                return alt_key;
            }
            _ => {
                for child in get_children() {
                    self.infer_expr(&child, &nested_path, &mut nested_fields);
                }
            }
        }

        if nested_fields.is_empty() {
            return type_annotation.map(TypeKey::Named).unwrap_or(TypeKey::Node);
        }

        let key = if let Some(name) = type_annotation {
            TypeKey::Named(name)
        } else {
            TypeKey::Synthetic(nested_path)
        };

        self.table
            .insert(key.clone(), TypeValue::Struct(nested_fields));
        key
    }

    fn infer_quantified(
        &mut self,
        quant: &ast::QuantifiedExpr,
        path: &[&'src str],
        fields: &mut IndexMap<&'src str, TypeKey<'src>>,
    ) -> Option<TypeKey<'src>> {
        let inner = quant.inner()?;
        let op = quant.operator()?;

        // If the inner is a capture, we need special handling for the wrapper
        if let Expr::CapturedExpr(cap) = &inner {
            let name_tok = cap.name()?;
            let capture_name = token_src(&name_tok, self.source);

            let type_annotation = cap.type_annotation().and_then(|t| {
                let tok = t.name()?;
                Some(token_src(&tok, self.source))
            });

            let inner_key =
                self.infer_capture_inner(cap.inner().as_ref(), path, capture_name, type_annotation);
            let wrapped_key = self.wrap_with_quantifier(&inner_key, op.kind());

            fields.insert(capture_name, wrapped_key.clone());
            return Some(wrapped_key);
        }

        // Non-capture quantified expression: recurse into inner
        self.infer_expr(&inner, path, fields)
    }

    fn wrap_with_quantifier(
        &mut self,
        inner: &TypeKey<'src>,
        op_kind: SyntaxKind,
    ) -> TypeKey<'src> {
        let wrapper = match op_kind {
            SyntaxKind::Question | SyntaxKind::QuestionQuestion => {
                TypeValue::Optional(inner.clone())
            }
            SyntaxKind::Star | SyntaxKind::StarQuestion => TypeValue::List(inner.clone()),
            SyntaxKind::Plus | SyntaxKind::PlusQuestion => TypeValue::NonEmptyList(inner.clone()),
            _ => return inner.clone(),
        };

        // Create a unique key for the wrapper
        let wrapper_key = match inner {
            TypeKey::Named(name) => {
                let suffix = match op_kind {
                    SyntaxKind::Question | SyntaxKind::QuestionQuestion => "Opt",
                    SyntaxKind::Star | SyntaxKind::StarQuestion => "List",
                    SyntaxKind::Plus | SyntaxKind::PlusQuestion => "List",
                    _ => "",
                };
                TypeKey::Synthetic(vec![name, suffix])
            }
            TypeKey::Synthetic(segments) => {
                let mut new_segments = segments.clone();
                let suffix = match op_kind {
                    SyntaxKind::Question | SyntaxKind::QuestionQuestion => "opt",
                    SyntaxKind::Star | SyntaxKind::StarQuestion => "list",
                    SyntaxKind::Plus | SyntaxKind::PlusQuestion => "list",
                    _ => "",
                };
                new_segments.push(suffix);
                TypeKey::Synthetic(new_segments)
            }
            _ => {
                // For builtins like Node, we directly store the wrapper under a synthetic key
                let type_name = match inner {
                    TypeKey::Node => "Node",
                    TypeKey::String => "String",
                    TypeKey::Unit => "Unit",
                    _ => "Unknown",
                };
                let suffix = match op_kind {
                    SyntaxKind::Question | SyntaxKind::QuestionQuestion => "Opt",
                    SyntaxKind::Star | SyntaxKind::StarQuestion => "List",
                    SyntaxKind::Plus | SyntaxKind::PlusQuestion => "List",
                    _ => "",
                };
                TypeKey::Synthetic(vec![type_name, suffix])
            }
        };

        self.table.insert(wrapper_key.clone(), wrapper);
        wrapper_key
    }

    fn infer_alt(
        &mut self,
        alt: &ast::AltExpr,
        path: &[&'src str],
        fields: &mut IndexMap<&'src str, TypeKey<'src>>,
    ) -> Option<TypeKey<'src>> {
        // Alt without capture: just collect fields from all branches into current scope
        match alt.kind() {
            AltKind::Tagged => {
                // Tagged alt without capture: unusual, but collect fields
                for branch in alt.branches() {
                    if let Some(body) = branch.body() {
                        self.infer_expr(&body, path, fields);
                    }
                }
            }
            AltKind::Untagged | AltKind::Mixed => {
                // Untagged alt: merge fields from branches
                let branch_fields = self.collect_branch_fields(alt, path);
                let merged = TypeTable::merge_fields(&branch_fields);
                self.apply_merged_fields(merged, fields, alt);
            }
        }
        None
    }

    fn infer_alt_as_type(
        &mut self,
        alt: &ast::AltExpr,
        path: &[&'src str],
        type_annotation: Option<&'src str>,
    ) -> TypeKey<'src> {
        match alt.kind() {
            AltKind::Tagged => self.infer_tagged_alt(alt, path, type_annotation),
            AltKind::Untagged | AltKind::Mixed => {
                self.infer_untagged_alt(alt, path, type_annotation)
            }
        }
    }

    fn infer_tagged_alt(
        &mut self,
        alt: &ast::AltExpr,
        path: &[&'src str],
        type_annotation: Option<&'src str>,
    ) -> TypeKey<'src> {
        let mut variants = IndexMap::new();

        for branch in alt.branches() {
            let Some(label_tok) = branch.label() else {
                continue;
            };
            let label = token_src(&label_tok, self.source);

            let mut variant_path = path.to_vec();
            variant_path.push(label);

            let mut variant_fields = IndexMap::new();
            if let Some(body) = branch.body() {
                self.infer_expr(&body, &variant_path, &mut variant_fields);
            }

            let variant_key = TypeKey::Synthetic(variant_path);
            let variant_value = if variant_fields.is_empty() {
                TypeValue::Unit
            } else {
                TypeValue::Struct(variant_fields)
            };

            self.table.insert(variant_key.clone(), variant_value);
            variants.insert(label, variant_key);
        }

        let key = if let Some(name) = type_annotation {
            TypeKey::Named(name)
        } else {
            TypeKey::Synthetic(path.to_vec())
        };

        self.table
            .insert(key.clone(), TypeValue::TaggedUnion(variants));
        key
    }

    fn infer_untagged_alt(
        &mut self,
        alt: &ast::AltExpr,
        path: &[&'src str],
        type_annotation: Option<&'src str>,
    ) -> TypeKey<'src> {
        let branch_fields = self.collect_branch_fields(alt, path);
        let merged = TypeTable::merge_fields(&branch_fields);

        if merged.is_empty() {
            return type_annotation.map(TypeKey::Named).unwrap_or(TypeKey::Node);
        }

        let mut result_fields = IndexMap::new();
        self.apply_merged_fields(merged, &mut result_fields, alt);

        let key = if let Some(name) = type_annotation {
            TypeKey::Named(name)
        } else {
            TypeKey::Synthetic(path.to_vec())
        };

        self.table
            .insert(key.clone(), TypeValue::Struct(result_fields));
        key
    }

    fn collect_branch_fields(
        &mut self,
        alt: &ast::AltExpr,
        path: &[&'src str],
    ) -> Vec<IndexMap<&'src str, TypeKey<'src>>> {
        let mut branch_fields = Vec::new();

        for branch in alt.branches() {
            let mut fields = IndexMap::new();
            if let Some(body) = branch.body() {
                self.infer_expr(&body, path, &mut fields);
            }
            branch_fields.push(fields);
        }

        branch_fields
    }

    fn apply_merged_fields(
        &mut self,
        merged: IndexMap<&'src str, MergedField<'src>>,
        fields: &mut IndexMap<&'src str, TypeKey<'src>>,
        alt: &ast::AltExpr,
    ) {
        for (name, merge_result) in merged {
            let key = match merge_result {
                MergedField::Same(k) => k,
                MergedField::Optional(k) => {
                    let wrapper_key = TypeKey::Synthetic(vec![name, "opt"]);
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
            fields.insert(name, key);
        }
    }
}
