//! Dump helpers for type inference inspection and testing.

use std::fmt::Write;

use crate::ir::{TYPE_NODE, TYPE_STR, TYPE_VOID, TypeId, TypeKind};

use super::infer::TypeInferenceResult;

impl TypeInferenceResult<'_> {
    pub fn dump(&self) -> String {
        let mut out = String::new();
        let printer = TypePrinter::new(self);
        printer.format(&mut out).expect("String write never fails");
        out
    }

    pub fn dump_diagnostics(&self, source: &str) -> String {
        self.diagnostics.render_filtered(source)
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

struct TypePrinter<'a, 'src> {
    result: &'a TypeInferenceResult<'src>,
    width: usize,
}

impl<'a, 'src> TypePrinter<'a, 'src> {
    fn new(result: &'a TypeInferenceResult<'src>) -> Self {
        let total_types = 3 + result.type_defs.len();
        let width = if total_types == 0 {
            1
        } else {
            ((total_types as f64).log10().floor() as usize) + 1
        };
        Self { result, width }
    }

    fn format(&self, w: &mut String) -> std::fmt::Result {
        // Entrypoints (skip redundant Foo = Foo)
        for (name, type_id) in &self.result.entrypoint_types {
            let type_name = self.get_type_name(*type_id);
            if type_name.as_deref() == Some(*name) {
                continue;
            }
            writeln!(w, "{} = {}", name, self.format_type(*type_id))?;
        }

        let has_entrypoints = self
            .result
            .entrypoint_types
            .iter()
            .any(|(name, id)| self.get_type_name(*id).as_deref() != Some(*name));

        // Type definitions (skip inlinable types)
        let mut first_typedef = true;
        for (idx, def) in self.result.type_defs.iter().enumerate() {
            let type_id = 3 + idx as TypeId;

            if self.is_inlinable(type_id) {
                continue;
            }

            if first_typedef && has_entrypoints {
                writeln!(w)?;
            }
            first_typedef = false;

            let header = self.format_type_header(type_id, def.name);

            match def.kind {
                TypeKind::Record => {
                    if def.members.len() == 1 {
                        let m = &def.members[0];
                        writeln!(
                            w,
                            "{} = {{ {}: {} }}",
                            header,
                            m.name,
                            self.format_type(m.ty)
                        )?;
                    } else {
                        writeln!(w, "{} = {{", header)?;
                        for member in &def.members {
                            writeln!(w, "  {}: {}", member.name, self.format_type(member.ty))?;
                        }
                        writeln!(w, "}}")?;
                    }
                }
                TypeKind::Enum => {
                    if def.members.len() == 1 {
                        let m = &def.members[0];
                        writeln!(
                            w,
                            "{} = {{ {} => {} }}",
                            header,
                            m.name,
                            self.format_type(m.ty)
                        )?;
                    } else {
                        writeln!(w, "{} = {{", header)?;
                        for member in &def.members {
                            writeln!(w, "  {} => {}", member.name, self.format_type(member.ty))?;
                        }
                        writeln!(w, "}}")?;
                    }
                }
                TypeKind::Optional => {
                    let inner = def
                        .inner_type
                        .map(|t| self.format_type(t))
                        .unwrap_or_default();
                    writeln!(w, "{} = {}?", header, inner)?;
                }
                TypeKind::ArrayStar => {
                    let inner = def
                        .inner_type
                        .map(|t| self.format_type(t))
                        .unwrap_or_default();
                    writeln!(w, "{} = [{}]", header, inner)?;
                }
                TypeKind::ArrayPlus => {
                    let inner = def
                        .inner_type
                        .map(|t| self.format_type(t))
                        .unwrap_or_default();
                    writeln!(w, "{} = [{}]⁺", header, inner)?;
                }
            }
        }

        // Errors
        if !self.result.errors.is_empty() {
            if has_entrypoints || !first_typedef {
                writeln!(w)?;
            }
            writeln!(w, "Errors:")?;
            for err in &self.result.errors {
                let types = err
                    .types_found
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(
                    w,
                    "  field `{}` in `{}`: incompatible types [{}]",
                    err.field, err.definition, types
                )?;
            }
        }

        Ok(())
    }

    fn get_type_name(&self, id: TypeId) -> Option<String> {
        if id < 3 {
            return None;
        }
        let idx = (id - 3) as usize;
        self.result
            .type_defs
            .get(idx)
            .and_then(|def| def.name.map(|s| s.to_string()))
    }

    /// Returns true if the type should be inlined rather than shown as separate definition.
    /// Inlinable: wrapper types (Optional/Array*) around primitives or other inlinable types.
    fn is_inlinable(&self, id: TypeId) -> bool {
        if id < 3 {
            return true; // primitives are always inlinable
        }
        let idx = (id - 3) as usize;
        let Some(def) = self.result.type_defs.get(idx) else {
            return false;
        };

        // Named types are not inlined (they have semantic meaning)
        if def.name.is_some() {
            return false;
        }

        match def.kind {
            TypeKind::Record | TypeKind::Enum => false,
            TypeKind::Optional | TypeKind::ArrayStar | TypeKind::ArrayPlus => {
                def.inner_type.map(|t| self.is_inlinable(t)).unwrap_or(true)
            }
        }
    }

    fn format_type_header(&self, type_id: TypeId, name: Option<&str>) -> String {
        match name {
            Some(n) => n.to_string(),
            None => format!("T{:0width$}", type_id, width = self.width),
        }
    }

    fn format_type(&self, id: TypeId) -> String {
        match id {
            TYPE_VOID => "()".to_string(),
            TYPE_NODE => "Node".to_string(),
            TYPE_STR => "str".to_string(),
            _ => {
                let idx = (id - 3) as usize;
                if let Some(def) = self.result.type_defs.get(idx) {
                    // Named types: use name
                    if let Some(name) = def.name {
                        return name.to_string();
                    }

                    // Inlinable wrappers: format inline
                    if self.is_inlinable(id) {
                        let inner = def
                            .inner_type
                            .map(|t| self.format_type(t))
                            .unwrap_or_default();
                        return match def.kind {
                            TypeKind::Optional => format!("{}?", inner),
                            TypeKind::ArrayStar => format!("[{}]", inner),
                            TypeKind::ArrayPlus => format!("[{}]⁺", inner),
                            _ => format!("T{:0width$}", id, width = self.width),
                        };
                    }
                }
                format!("T{:0width$}", id, width = self.width)
            }
        }
    }
}
