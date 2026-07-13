//! Type table accumulator for bytecode emission.
//!
//! Owns the bytecode-level type tables (defs, members, names) and the query →
//! bytecode id mapping, plus the primitive push/resolve operations the type-emit
//! phase drives. The walk that decides reachability, ordering, and how each
//! inferred shape lowers lives in `compiler::emit::type_table`.

use std::collections::HashMap;

use crate::bytecode::{TypeDef, TypeId as WireTypeId, TypeMember, TypeNameEntry};

use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::DefinitionOutput;
use crate::compiler::ids::TypeId;

use super::error::EmitError;

/// Holds the type metadata, mapping query TypeIds to wire TypeIds.
#[derive(Debug)]
pub struct TypeTableBuilder {
    /// Map from query TypeId to bytecode WireTypeId.
    mapping: HashMap<TypeId, WireTypeId>,
    /// Type definitions (4 bytes each).
    type_defs: Vec<TypeDef>,
    /// Type members for records/variants (4 bytes each).
    type_members: Vec<TypeMember>,
    /// Type names for named types (4 bytes each).
    type_names: Vec<TypeNameEntry>,
    no_value: Option<WireTypeId>,
}

impl TypeTableBuilder {
    pub fn new() -> Self {
        Self {
            mapping: HashMap::new(),
            type_defs: Vec::new(),
            type_members: Vec::new(),
            type_names: Vec::new(),
            no_value: None,
        }
    }

    /// Map `query_id` to the next bytecode slot and push `def`; returns the id.
    /// Used to emit builtins and to reserve placeholder slots for custom types.
    pub fn push_mapped(&mut self, query_id: TypeId, def: TypeDef) -> Result<WireTypeId, EmitError> {
        if self.type_defs.len() >= EmitError::MAX_TYPES {
            return Err(EmitError::TooManyTypes(self.type_defs.len() + 1));
        }

        let bc_id = WireTypeId::from(self.type_defs.len() as u16);
        self.mapping.insert(query_id, bc_id);
        self.type_defs.push(def);
        Ok(bc_id)
    }

    pub fn push_no_value(&mut self) -> Result<WireTypeId, EmitError> {
        if self.type_defs.len() >= EmitError::MAX_TYPES {
            return Err(EmitError::TooManyTypes(self.type_defs.len() + 1));
        }
        let wire_id = WireTypeId::from(self.type_defs.len() as u16);
        self.type_defs
            .push(TypeDef::builtin(crate::bytecode::TypeKind::NoValue));
        self.no_value = Some(wire_id);
        Ok(wire_id)
    }

    pub fn resolve_output(
        &self,
        output: DefinitionOutput,
        type_ctx: &TypeAnalysis,
    ) -> Result<WireTypeId, EmitError> {
        match output {
            DefinitionOutput::MatchOnly => Ok(self
                .no_value
                .expect("match-only output requires a bytecode no-value type")),
            DefinitionOutput::Value(type_id) => self.resolve_type(type_id, type_ctx),
        }
    }

    /// Overwrite a previously reserved slot with its final definition.
    pub fn fill_slot(&mut self, slot_index: usize, def: TypeDef) {
        self.type_defs[slot_index] = def;
    }

    /// Member-table length: the base index for the next record or variant type's members.
    pub fn members_len(&self) -> u16 {
        u16::try_from(self.type_members.len())
            .expect("capture layout validates the type-member index space")
    }

    pub fn push_member(&mut self, member: TypeMember) {
        self.type_members.push(member);
    }

    pub fn push_name(&mut self, entry: TypeNameEntry) -> Result<(), EmitError> {
        if self.type_names.len() >= EmitError::MAX_TYPE_NAMES {
            return Err(EmitError::TooManyTypeNames(self.type_names.len() + 1));
        }

        self.type_names.push(entry);
        Ok(())
    }

    /// Resolve a query TypeId to its underlying bytecode WireTypeId.
    ///
    /// Ref types are emitted as aliases only when they are definition results. In
    /// every materialized position, follow the reference chain to the actual shape.
    pub fn resolve_type(
        &self,
        type_id: TypeId,
        type_ctx: &TypeAnalysis,
    ) -> Result<WireTypeId, EmitError> {
        let type_id = type_ctx.resolve_underlying_type_id(type_id);
        let bc_id = self
            .mapping
            .get(&type_id)
            .copied()
            .expect("resolved type must be mapped");
        Ok(bc_id)
    }

    /// Validate that counts fit in u16.
    pub fn validate(&self) -> Result<(), EmitError> {
        if self.type_defs.len() > EmitError::MAX_TYPES {
            return Err(EmitError::TooManyTypes(self.type_defs.len()));
        }
        if self.type_names.len() > EmitError::MAX_TYPE_NAMES {
            return Err(EmitError::TooManyTypeNames(self.type_names.len()));
        }
        Ok(())
    }

    pub fn lookup(&self, type_id: TypeId) -> Option<WireTypeId> {
        self.mapping.get(&type_id).copied()
    }

    /// Returns `(type_defs_bytes, type_members_bytes, type_names_bytes)`.
    pub fn emit(&self) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut defs_bytes = Vec::with_capacity(self.type_defs.len() * TypeDef::SIZE);
        for def in &self.type_defs {
            defs_bytes.extend_from_slice(&def.to_bytes());
        }

        let mut members_bytes = Vec::with_capacity(self.type_members.len() * TypeMember::SIZE);
        for member in &self.type_members {
            members_bytes.extend_from_slice(&member.to_bytes());
        }

        let mut names_bytes = Vec::with_capacity(self.type_names.len() * TypeNameEntry::SIZE);
        for type_name in &self.type_names {
            names_bytes.extend_from_slice(&type_name.to_bytes());
        }

        (defs_bytes, members_bytes, names_bytes)
    }

    /// Number of type definitions.
    pub fn type_defs_count(&self) -> usize {
        self.type_defs.len()
    }

    /// Number of type members.
    pub fn type_members_count(&self) -> usize {
        self.type_members.len()
    }

    /// Number of type names.
    pub fn type_names_count(&self) -> usize {
        self.type_names.len()
    }
}

impl Default for TypeTableBuilder {
    fn default() -> Self {
        Self::new()
    }
}
