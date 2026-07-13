//! Debug-only type verification for materialized values.
//!
//! Verifies that materialized `Value` matches the declared `result_type` from bytecode.
//! Zero-cost in release builds.

use crate::bytecode::{Module, TypeId};
#[cfg(debug_assertions)]
use crate::bytecode::{StringsView, TypeDefKind, TypeKind, TypesView};
use crate::core::Colors;

use super::Value;

/// Panics with a pretty diagnostic if the value doesn't match the declared type.
/// `declared_type` should be the `result_type` from the entry point that was executed.
/// No-op in release builds.
#[cfg(debug_assertions)]
pub fn debug_verify_type(value: &Value, declared_type: TypeId, module: &Module, colors: Colors) {
    let mut verifier = TypeVerifier::new(module);
    verifier.verify(value, declared_type);
    if !verifier.errors.is_empty() {
        panic_with_mismatch(value, declared_type, &verifier.errors, module, colors);
    }
}

#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn debug_verify_type(
    _value: &Value,
    _declared_type: TypeId,
    _module: &Module,
    _colors: Colors,
) {
}

/// Walks a materialized `Value` against its declared type, accumulating
/// mismatches. Bundles the always-paired `(TypesView, StringsView)` and owns the
/// recursion's `path` cursor and `errors` sink so the walker needs no out-params.
#[cfg(debug_assertions)]
struct TypeVerifier<'a> {
    types: TypesView<'a>,
    strings: StringsView<'a>,
    path: String,
    errors: Vec<String>,
    depth: u32,
}

/// Native-recursion bound for the verifier — the one remaining recursive walk over
/// a materialized value. The cap keeps a pathologically deep value from overflowing
/// the stack in debug builds, so it must sit well under the real limit (debug
/// frames here are fat; the stack gives out around ~2500 of them). Past it,
/// verification simply stops: a type-soundness bug deeper than the cap goes
/// unchecked. That is an accepted trade, not a guarantee of completeness — the
/// verifier is debug-only defense-in-depth (release builds skip it entirely) and
/// such bugs almost always surface shallow.
#[cfg(debug_assertions)]
const MAX_VERIFY_DEPTH: u32 = 512;

#[cfg(debug_assertions)]
impl<'a> TypeVerifier<'a> {
    fn new(module: &'a Module) -> Self {
        Self {
            types: module.types(),
            strings: module.strings(),
            path: String::new(),
            errors: Vec::new(),
            depth: 0,
        }
    }

    /// Depth-guarded entry; every recursive descent goes through here.
    fn verify(&mut self, value: &Value, declared: TypeId) {
        if self.depth >= MAX_VERIFY_DEPTH {
            return;
        }
        self.depth += 1;
        self.verify_inner(value, declared);
        self.depth -= 1;
    }

    fn verify_inner(&mut self, value: &Value, declared: TypeId) {
        let Some(type_def) = self.types.get(declared) else {
            self.errors.push(format_error(
                &self.path,
                &format!("unknown type id {declared}"),
            ));
            return;
        };

        match type_def.decode() {
            TypeDefKind::Primitive(kind) => match kind {
                TypeKind::NoValue => {
                    if !matches!(value, Value::Null) {
                        self.errors.push(format_error(
                            &self.path,
                            &format!("type: no value, value: {}", value_kind_name(value)),
                        ));
                    }
                }
                TypeKind::Node => {
                    if !matches!(value, Value::Node(_)) {
                        self.errors.push(format_error(
                            &self.path,
                            &format!("type: Node, value: {}", value_kind_name(value)),
                        ));
                    }
                }
                TypeKind::Text => {
                    if !matches!(value, Value::Text(_)) {
                        self.errors.push(format_error(
                            &self.path,
                            &format!("type: Text, value: {}", value_kind_name(value)),
                        ));
                    }
                }
                TypeKind::Bool => {
                    if !matches!(value, Value::Bool(_)) {
                        self.errors.push(format_error(
                            &self.path,
                            &format!("type: Bool, value: {}", value_kind_name(value)),
                        ));
                    }
                }
                _ => unreachable!(),
            },

            TypeDefKind::Wrapper { kind, inner } => match kind {
                TypeKind::Alias => {
                    self.verify(value, inner);
                }
                TypeKind::Option => {
                    if !matches!(value, Value::Null) {
                        self.verify(value, inner);
                    }
                }
                TypeKind::ListZeroOrMore => match value {
                    Value::List(items) => {
                        for (i, item) in items.iter().enumerate() {
                            let prev_len = self.path.len();
                            self.path.push_str(&format!("[{}]", i));
                            self.verify(item, inner);
                            self.path.truncate(prev_len);
                        }
                    }
                    _ => {
                        self.errors.push(format_error(
                            &self.path,
                            &format!("type: list, value: {}", value_kind_name(value)),
                        ));
                    }
                },
                TypeKind::ListOneOrMore => match value {
                    Value::List(items) => {
                        if items.is_empty() {
                            self.errors.push(format_error(
                                &self.path,
                                "type: list with minimum one item, value: empty list",
                            ));
                        }
                        for (i, item) in items.iter().enumerate() {
                            let prev_len = self.path.len();
                            self.path.push_str(&format!("[{}]", i));
                            self.verify(item, inner);
                            self.path.truncate(prev_len);
                        }
                    }
                    _ => {
                        self.errors.push(format_error(
                            &self.path,
                            &format!("type: list, value: {}", value_kind_name(value)),
                        ));
                    }
                },
                _ => unreachable!(),
            },

            TypeDefKind::Record { .. } => match value {
                Value::Record(fields) => {
                    // Collect first: `members_of` borrows `self.types`, which would
                    // clash with the `&mut self` recursion inside the loop.
                    let members: Vec<_> = self.types.members_of(&type_def).collect();
                    for member in members {
                        let field_name = self.strings.get(member.name_id);
                        let field_value = fields.iter().find(|(k, _)| *k == field_name);
                        match field_value {
                            Some((_, v)) => {
                                let field_type = match (self.types.option_inner(member.type_id), v)
                                {
                                    (Some(_), Value::Null) => continue,
                                    (Some(inner), _) => inner,
                                    (None, _) => member.type_id,
                                };
                                let prev_len = self.path.len();
                                self.path.push('.');
                                self.path.push_str(field_name);
                                self.verify(v, field_type);
                                self.path.truncate(prev_len);
                            }
                            None => {
                                // Policy: every declared field is always present in
                                // the output — option-typed fields materialize as null,
                                // never as an absent key. A missing key is always a bug.
                                self.errors.push(format!(
                                        "{}: field missing (declared fields are always present; option values use null)",
                                        append_path(&self.path, field_name)
                                    ));
                            }
                        }
                    }
                }
                _ => {
                    self.errors.push(format_error(
                        &self.path,
                        &format!("type: record, value: {}", value_kind_name(value)),
                    ));
                }
            },
            TypeDefKind::Variant { .. } => match value {
                Value::Variant { tag, data } => {
                    let case = self
                        .types
                        .members_of(&type_def)
                        .find(|m| self.strings.get(m.name_id) == *tag);

                    match case {
                        Some(member) => {
                            let has_no_payload = self.types.get(member.type_id).is_some_and(|d| {
                                matches!(d.decode(), TypeDefKind::Primitive(TypeKind::NoValue))
                            });

                            if has_no_payload {
                                if data.is_some() {
                                    self.errors.push(format!(
                                        "{}: no-payload case '{}' should have no $data",
                                        append_path(&self.path, "$data"),
                                        tag
                                    ));
                                }
                            } else {
                                match data {
                                    Some(d) => {
                                        let prev_len = self.path.len();
                                        self.path.push_str(".$data");
                                        self.verify(d, member.type_id);
                                        self.path.truncate(prev_len);
                                    }
                                    None => {
                                        self.errors.push(format!(
                                            "{}: payload-bearing case '{}' should have $data",
                                            append_path(&self.path, "$data"),
                                            tag
                                        ));
                                    }
                                }
                            }
                        }
                        None => {
                            self.errors.push(format!(
                                "{}: unknown case '{}'",
                                append_path(&self.path, "$tag"),
                                tag
                            ));
                        }
                    }
                }
                _ => {
                    self.errors.push(format_error(
                        &self.path,
                        &format!("type: variant, value: {}", value_kind_name(value)),
                    ));
                }
            },
        }
    }
}

#[cfg(debug_assertions)]
fn value_kind_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Node(_) => "Node",
        Value::Text(_) => "Text",
        Value::Bool(_) => "Bool",
        Value::List(_) => "list",
        Value::Record(_) => "record",
        Value::Variant { .. } => "variant",
    }
}

/// Strips leading `.` so the root path prints as empty rather than `.`.
#[cfg(debug_assertions)]
fn format_path(path: &str) -> String {
    path.strip_prefix('.').unwrap_or(path).to_string()
}

#[cfg(debug_assertions)]
fn format_error(path: &str, msg: &str) -> String {
    let p = format_path(path);
    if p.is_empty() {
        msg.to_string()
    } else {
        format!("{}: {}", p, msg)
    }
}

#[cfg(debug_assertions)]
fn append_path(path: &str, suffix: &str) -> String {
    let p = format_path(path);
    if p.is_empty() {
        suffix.to_string()
    } else {
        format!("{}.{}", p, suffix)
    }
}

#[cfg(debug_assertions)]
fn centered_header(label: &str, width: usize) -> String {
    let label_with_spaces = format!(" {} ", label);
    let label_len = label_with_spaces.len();
    if label_len >= width {
        return label_with_spaces;
    }
    let remaining = width - label_len;
    let left = remaining / 2;
    let right = remaining - left;
    format!(
        "{}{}{}",
        "-".repeat(left),
        label_with_spaces,
        "-".repeat(right)
    )
}

#[cfg(debug_assertions)]
fn panic_with_mismatch(
    value: &Value,
    declared_type: TypeId,
    errors: &[String],
    module: &Module,
    colors: Colors,
) -> ! {
    const WIDTH: usize = 80;
    let separator = "=".repeat(WIDTH);

    let entry_points = module.entry_points();
    let strings = module.strings();

    let type_name = entry_points
        .iter()
        .find_map(|e| {
            if e.result_type() == declared_type {
                Some(strings.get(e.name()))
            } else {
                None
            }
        })
        .unwrap_or("unknown");

    let value_str = value.format(true, colors);
    let details_str = errors.join("\n");

    let output_header = centered_header(&format!("Output: {}", type_name), WIDTH);
    let details_header = centered_header("Details", WIDTH);

    panic!(
        "\n{separator}\n\
         BUG: Type and value do not match\n\
         {separator}\n\n\
         {output_header}\n\n\
         {value_str}\n\n\
         {details_header}\n\n\
         {details_str}\n\n\
         {separator}\n"
    );
}
