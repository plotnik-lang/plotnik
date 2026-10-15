use crate::bytecode::{TypeDef, TypeId as WireTypeId, TypeMember, TypeNameEntry};

use crate::compiler::analyze::result::ResultTypeLayout;
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::DefinitionOutput;
use crate::compiler::ids::{ResultTypeId, TypeId};

use super::error::EmitError;

#[derive(Debug)]
pub struct TypeTableBuilder {
    type_defs: Vec<TypeDef>,
    type_members: Vec<TypeMember>,
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

    pub fn fill_slot(&mut self, slot_index: usize, def: TypeDef) {
        self.type_defs[slot_index] = def;
    }

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

    pub fn type_defs_count(&self) -> usize {
        self.type_defs.len()
    }

    pub fn type_members_count(&self) -> usize {
        self.type_members.len()
    }

    pub fn type_names_count(&self) -> usize {
        self.type_names.len()
    }
}
