//! Type table accumulator for bytecode emission.
//!
//! Owns the bytecode-level type tables (defs, members, names) and the query →
//! bytecode id mapping, plus the primitive push/resolve operations the type-emit
//! phase drives. The walk that decides reachability, ordering, and how each
//! inferred shape lowers lives in `compiler::emit::build_types`.

use std::collections::HashMap;

use crate::bytecode::{TypeDef, TypeDefKind, TypeId as WireTypeId, TypeMember, TypeNameEntry};

use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::ids::TypeId;

use super::error::EmitError;

/// Holds the type metadata, mapping query TypeIds to wire TypeIds.
#[derive(Debug)]
pub struct TypeTableBuilder {
    /// Map from query TypeId to bytecode WireTypeId.
    mapping: HashMap<TypeId, WireTypeId>,
    /// Type definitions (4 bytes each).
    type_defs: Vec<TypeDef>,
    /// Type members for structs/enums (4 bytes each).
    type_members: Vec<TypeMember>,
    /// Type names for named types (4 bytes each).
    type_names: Vec<TypeNameEntry>,
    /// Cache for dynamically created Optional wrappers: base_type -> Optional(base_type)
    optional_wrappers: HashMap<WireTypeId, WireTypeId>,
}

impl TypeTableBuilder {
    pub fn new() -> Self {
        Self {
            mapping: HashMap::new(),
            type_defs: Vec::new(),
            type_members: Vec::new(),
            type_names: Vec::new(),
            optional_wrappers: HashMap::new(),
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

    /// Overwrite a previously reserved slot with its final definition.
    pub fn fill_slot(&mut self, slot_index: usize, def: TypeDef) {
        self.type_defs[slot_index] = def;
    }

    /// Member-table length, i.e. the base index for the next struct/enum's members.
    pub fn members_len(&self) -> Result<u16, EmitError> {
        u16::try_from(self.type_members.len())
            .map_err(|_| EmitError::TooManyTypeMembers(self.type_members.len()))
    }

    pub fn push_member(&mut self, member: TypeMember) -> Result<(), EmitError> {
        if self.type_members.len() >= EmitError::MAX_TYPE_MEMBERS {
            return Err(EmitError::TooManyTypeMembers(self.type_members.len() + 1));
        }

        self.type_members.push(member);
        Ok(())
    }

    pub fn push_name(&mut self, entry: TypeNameEntry) -> Result<(), EmitError> {
        if self.type_names.len() >= EmitError::MAX_TYPE_NAMES {
            return Err(EmitError::TooManyTypeNames(self.type_names.len() + 1));
        }

        self.type_names.push(entry);
        Ok(())
    }

    /// Intern an `Optional(base_type)` wrapper, deduplicating by base type.
    pub fn intern_optional(&mut self, base_type: WireTypeId) -> Result<WireTypeId, EmitError> {
        if let Some(&optional_id) = self.optional_wrappers.get(&base_type) {
            return Ok(optional_id);
        }

        if self.type_defs.len() >= EmitError::MAX_TYPES {
            return Err(EmitError::TooManyTypes(self.type_defs.len() + 1));
        }

        let optional_id = WireTypeId::from(self.type_defs.len() as u16);
        self.type_defs.push(TypeDef::optional(base_type));
        self.optional_wrappers.insert(base_type, optional_id);
        Ok(optional_id)
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
        if self.type_members.len() > EmitError::MAX_TYPE_MEMBERS {
            return Err(EmitError::TooManyTypeMembers(self.type_members.len()));
        }
        if self.type_names.len() > EmitError::MAX_TYPE_NAMES {
            return Err(EmitError::TooManyTypeNames(self.type_names.len()));
        }
        Ok(())
    }

    pub fn lookup(&self, type_id: TypeId) -> Option<WireTypeId> {
        self.mapping.get(&type_id).copied()
    }

    /// Get the absolute member base index for a struct/enum type.
    ///
    /// For Struct and Enum types, returns the starting index in the TypeMembers table.
    /// Fields/variants are at indices [base..base+count).
    pub fn member_base(&self, type_id: TypeId) -> Option<u16> {
        let bc_type_id = self.mapping.get(&type_id)?;
        let type_def = self.type_defs.get(u16::from(*bc_type_id) as usize)?;
        match type_def.decode() {
            TypeDefKind::Struct { member_start, .. } | TypeDefKind::Enum { member_start, .. } => {
                Some(member_start)
            }
            _ => None,
        }
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
