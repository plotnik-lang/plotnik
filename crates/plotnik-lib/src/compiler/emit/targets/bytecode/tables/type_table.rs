//! Type table accumulator for bytecode emission.
//!
//! Owns the bytecode-level type tables (defs, members, names) plus the primitive
//! push/resolve operations the type-emit phase drives. The target-neutral result
//! layout is the sole source of type IDs; this builder only validates and writes
//! their compact wire representation. The walk that decides reachability,
//! ordering, and how each inferred shape lowers lives in
//! `compiler::emit::type_table`.

use crate::bytecode::{TypeDef, TypeId as WireTypeId, TypeMember, TypeNameEntry};

use crate::compiler::analyze::result::ResultTypeLayout;
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::DefinitionOutput;
use crate::compiler::ids::{ResultTypeId, TypeId};

use super::error::EmitError;

/// Holds bytecode type metadata in canonical result-type order.
#[derive(Debug)]
pub struct TypeTableBuilder {
    /// Type definitions (4 bytes each).
    type_defs: Vec<TypeDef>,
    /// Type members for records/variants (4 bytes each).
    type_members: Vec<TypeMember>,
    /// Type names for named types (4 bytes each).
    type_names: Vec<TypeNameEntry>,
}

impl TypeTableBuilder {
    pub fn new() -> Self {
        Self {
            type_defs: Vec::new(),
            type_members: Vec::new(),
            type_names: Vec::new(),
        }
    }

    /// Push `def` at its canonical result-type slot and return the wire ID.
    pub fn push_result(
        &mut self,
        result_id: ResultTypeId,
        def: TypeDef,
    ) -> Result<WireTypeId, EmitError> {
        assert_eq!(
            result_id.index(),
            self.type_defs.len(),
            "bytecode types are emitted in canonical result-type order"
        );
        if self.type_defs.len() >= EmitError::MAX_TYPES {
            return Err(EmitError::TooManyTypes(self.type_defs.len() + 1));
        }

        let bc_id = Self::encode_wire_id(result_id)?;
        self.type_defs.push(def);
        Ok(bc_id)
    }

    pub fn resolve_output(
        &self,
        output: DefinitionOutput,
        type_ctx: &TypeAnalysis,
        layout: &ResultTypeLayout,
    ) -> Result<WireTypeId, EmitError> {
        match output {
            DefinitionOutput::MatchOnly => self.wire_id(layout.no_value_output_id()),
            DefinitionOutput::Value(type_id) => self.resolve_type(type_id, type_ctx, layout),
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

    /// Resolve a semantic type to the wire type used at a value position.
    /// Definition references use their underlying body; an explicit capture-type
    /// declaration preserves its alias identity.
    pub fn resolve_type(
        &self,
        type_id: TypeId,
        type_ctx: &TypeAnalysis,
        layout: &ResultTypeLayout,
    ) -> Result<WireTypeId, EmitError> {
        let type_id = match type_ctx.type_shape(type_id) {
            Some(crate::compiler::analyze::types::type_shape::TypeShape::Ref(declaration))
                if type_ctx.declaration_definition(*declaration).is_none() =>
            {
                type_id
            }
            _ => type_ctx.resolve_underlying_type_id(type_id),
        };
        self.wire_id(layout.output_id(type_id))
    }

    pub fn wire_id(&self, result_id: ResultTypeId) -> Result<WireTypeId, EmitError> {
        assert!(
            result_id.index() < self.type_defs.len(),
            "canonical result type must have a reserved bytecode slot"
        );
        Self::encode_wire_id(result_id)
    }

    fn encode_wire_id(result_id: ResultTypeId) -> Result<WireTypeId, EmitError> {
        let count = result_id.index() + 1;
        if count > EmitError::MAX_TYPES {
            return Err(EmitError::TooManyTypes(count));
        }
        let raw = u16::try_from(result_id.raw())
            .expect("bytecode type count was checked against the u16 format limit");
        Ok(WireTypeId::from(raw))
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
