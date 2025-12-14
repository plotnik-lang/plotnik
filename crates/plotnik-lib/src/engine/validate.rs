//! Runtime validation of query results against type metadata.
//!
//! Validates that `Value` produced by the materializer matches the expected
//! type from the IR. A mismatch indicates an IR construction bug.

use std::fmt;

use crate::ir::{
    CompiledQuery, TYPE_COMPOSITE_START, TYPE_NODE, TYPE_STR, TYPE_VOID, TypeId, TypeKind,
};

use super::value::Value;

/// Error returned when validation fails.
#[derive(Debug)]
pub struct TypeError {
    pub expected: TypeDescription,
    pub actual: TypeDescription,
    pub path: Vec<PathSegment>,
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "type mismatch at ")?;
        if self.path.is_empty() {
            write!(f, "<root>")?;
        } else {
            for (i, seg) in self.path.iter().enumerate() {
                if i > 0 {
                    write!(f, ".")?;
                }
                match seg {
                    PathSegment::Field(name) => write!(f, "{}", name)?,
                    PathSegment::Index(i) => write!(f, "[{}]", i)?,
                    PathSegment::Variant(tag) => write!(f, "<{}>", tag)?,
                }
            }
        }
        write!(f, ": expected {}, got {}", self.expected, self.actual)
    }
}

/// Segment in the path to a type error.
#[derive(Debug, Clone)]
pub enum PathSegment {
    Field(String),
    Index(usize),
    Variant(String),
}

/// Human-readable type description for error messages.
#[derive(Debug, Clone)]
pub enum TypeDescription {
    Void,
    Node,
    String,
    Optional(Box<TypeDescription>),
    Array(Box<TypeDescription>),
    NonEmptyArray(Box<TypeDescription>),
    Record(String),
    Enum(String),
    // Actual value descriptions
    ActualNull,
    ActualNode,
    ActualString,
    ActualArray(usize),
    ActualObject,
    ActualVariant(String),
}

impl fmt::Display for TypeDescription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeDescription::Void => write!(f, "void"),
            TypeDescription::Node => write!(f, "Node"),
            TypeDescription::String => write!(f, "string"),
            TypeDescription::Optional(inner) => write!(f, "{}?", inner),
            TypeDescription::Array(inner) => write!(f, "{}*", inner),
            TypeDescription::NonEmptyArray(inner) => write!(f, "{}+", inner),
            TypeDescription::Record(name) => write!(f, "struct {}", name),
            TypeDescription::Enum(name) => write!(f, "enum {}", name),
            TypeDescription::ActualNull => write!(f, "null"),
            TypeDescription::ActualNode => write!(f, "Node"),
            TypeDescription::ActualString => write!(f, "string"),
            TypeDescription::ActualArray(len) => write!(f, "array[{}]", len),
            TypeDescription::ActualObject => write!(f, "object"),
            TypeDescription::ActualVariant(tag) => write!(f, "variant({})", tag),
        }
    }
}

/// Validates a value against the expected type.
pub fn validate(
    value: &Value<'_>,
    expected: TypeId,
    query: &CompiledQuery,
) -> Result<(), TypeError> {
    let mut ctx = ValidationContext {
        query,
        path: Vec::new(),
    };
    ctx.validate_value(value, expected)
}

struct ValidationContext<'a> {
    query: &'a CompiledQuery,
    path: Vec<PathSegment>,
}

impl ValidationContext<'_> {
    fn validate_value(&mut self, value: &Value<'_>, expected: TypeId) -> Result<(), TypeError> {
        match expected {
            TYPE_VOID => self.expect_null(value),
            TYPE_NODE => self.expect_node(value),
            TYPE_STR => self.expect_string(value),
            id if id >= TYPE_COMPOSITE_START => self.validate_composite(value, id),
            _ => Ok(()), // Unknown primitive, skip validation
        }
    }

    fn expect_null(&self, value: &Value<'_>) -> Result<(), TypeError> {
        match value {
            Value::Null => Ok(()),
            _ => Err(self.type_error(TypeDescription::Void, self.describe_value(value))),
        }
    }

    fn expect_node(&self, value: &Value<'_>) -> Result<(), TypeError> {
        match value {
            Value::Node(_) => Ok(()),
            _ => Err(self.type_error(TypeDescription::Node, self.describe_value(value))),
        }
    }

    fn expect_string(&self, value: &Value<'_>) -> Result<(), TypeError> {
        match value {
            Value::String(_) => Ok(()),
            _ => Err(self.type_error(TypeDescription::String, self.describe_value(value))),
        }
    }

    fn validate_composite(&mut self, value: &Value<'_>, type_id: TypeId) -> Result<(), TypeError> {
        let idx = (type_id - TYPE_COMPOSITE_START) as usize;
        let Some(def) = self.query.type_defs().get(idx) else {
            return Ok(()); // Unknown type, skip
        };

        match def.kind {
            TypeKind::Optional => self.validate_optional(value, def.inner_type().unwrap()),
            TypeKind::ArrayStar => self.validate_array(value, def.inner_type().unwrap(), false),
            TypeKind::ArrayPlus => self.validate_array(value, def.inner_type().unwrap(), true),
            TypeKind::Record => self.validate_record(value, type_id, def),
            TypeKind::Enum => self.validate_enum(value, type_id, def),
        }
    }

    fn validate_optional(&mut self, value: &Value<'_>, inner: TypeId) -> Result<(), TypeError> {
        match value {
            Value::Null => Ok(()),
            _ => self.validate_value(value, inner),
        }
    }

    fn validate_array(
        &mut self,
        value: &Value<'_>,
        element: TypeId,
        non_empty: bool,
    ) -> Result<(), TypeError> {
        let Value::Array(items) = value else {
            let expected = if non_empty {
                TypeDescription::NonEmptyArray(Box::new(self.describe_type(element)))
            } else {
                TypeDescription::Array(Box::new(self.describe_type(element)))
            };
            return Err(self.type_error(expected, self.describe_value(value)));
        };

        if non_empty && items.is_empty() {
            return Err(self.type_error(
                TypeDescription::NonEmptyArray(Box::new(self.describe_type(element))),
                TypeDescription::ActualArray(0),
            ));
        }

        for (i, item) in items.iter().enumerate() {
            self.path.push(PathSegment::Index(i));
            self.validate_value(item, element)?;
            self.path.pop();
        }

        Ok(())
    }

    fn validate_record(
        &mut self,
        value: &Value<'_>,
        type_id: TypeId,
        def: &crate::ir::TypeDef,
    ) -> Result<(), TypeError> {
        let Value::Object(fields) = value else {
            return Err(self.type_error(self.describe_type(type_id), self.describe_value(value)));
        };

        let Some(members_slice) = def.members_slice() else {
            return Ok(());
        };
        let members = self.query.resolve_type_members(members_slice);

        for member in members {
            let field_name = self.query.string(member.name);
            self.path.push(PathSegment::Field(field_name.to_string()));

            // Field ID in the object is the index, need to find it
            if let Some(field_value) = fields.get(&member.name) {
                self.validate_value(field_value, member.ty)?;
            }
            // Missing field is OK if it's optional (would be Null)

            self.path.pop();
        }

        Ok(())
    }

    fn validate_enum(
        &mut self,
        value: &Value<'_>,
        type_id: TypeId,
        def: &crate::ir::TypeDef,
    ) -> Result<(), TypeError> {
        let Value::Variant { tag, value: inner } = value else {
            return Err(self.type_error(self.describe_type(type_id), self.describe_value(value)));
        };

        let Some(members_slice) = def.members_slice() else {
            return Ok(());
        };
        let members = self.query.resolve_type_members(members_slice);

        // Find the variant by tag
        let variant = members.iter().find(|m| m.name == *tag);
        let Some(variant) = variant else {
            // Unknown variant tag
            let tag_name = self.query.string(*tag);
            return Err(self.type_error(
                self.describe_type(type_id),
                TypeDescription::ActualVariant(tag_name.to_string()),
            ));
        };

        let tag_name = self.query.string(variant.name);
        self.path.push(PathSegment::Variant(tag_name.to_string()));
        self.validate_value(inner, variant.ty)?;
        self.path.pop();

        Ok(())
    }

    fn describe_type(&self, type_id: TypeId) -> TypeDescription {
        match type_id {
            TYPE_VOID => TypeDescription::Void,
            TYPE_NODE => TypeDescription::Node,
            TYPE_STR => TypeDescription::String,
            id if id >= TYPE_COMPOSITE_START => {
                let idx = (id - TYPE_COMPOSITE_START) as usize;
                if let Some(def) = self.query.type_defs().get(idx) {
                    match def.kind {
                        TypeKind::Optional => TypeDescription::Optional(Box::new(
                            self.describe_type(def.inner_type().unwrap()),
                        )),
                        TypeKind::ArrayStar => TypeDescription::Array(Box::new(
                            self.describe_type(def.inner_type().unwrap()),
                        )),
                        TypeKind::ArrayPlus => TypeDescription::NonEmptyArray(Box::new(
                            self.describe_type(def.inner_type().unwrap()),
                        )),
                        TypeKind::Record => {
                            let name = if def.name != crate::ir::STRING_NONE {
                                self.query.string(def.name).to_string()
                            } else {
                                format!("T{}", type_id)
                            };
                            TypeDescription::Record(name)
                        }
                        TypeKind::Enum => {
                            let name = if def.name != crate::ir::STRING_NONE {
                                self.query.string(def.name).to_string()
                            } else {
                                format!("T{}", type_id)
                            };
                            TypeDescription::Enum(name)
                        }
                    }
                } else {
                    TypeDescription::Node
                }
            }
            _ => TypeDescription::Node,
        }
    }

    fn describe_value(&self, value: &Value<'_>) -> TypeDescription {
        match value {
            Value::Null => TypeDescription::ActualNull,
            Value::Node(_) => TypeDescription::ActualNode,
            Value::String(_) => TypeDescription::ActualString,
            Value::Array(items) => TypeDescription::ActualArray(items.len()),
            Value::Object(_) => TypeDescription::ActualObject,
            Value::Variant { tag, .. } => {
                let tag_name = self.query.string(*tag);
                TypeDescription::ActualVariant(tag_name.to_string())
            }
        }
    }

    fn type_error(&self, expected: TypeDescription, actual: TypeDescription) -> TypeError {
        TypeError {
            expected,
            actual,
            path: self.path.clone(),
        }
    }
}
