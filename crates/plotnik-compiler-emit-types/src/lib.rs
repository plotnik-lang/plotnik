#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Type-table emission phase: lower the inferred types into the bytecode type
//! table, interning their names into the shared string table.

use plotnik_compiler_core::{EmitError, EmitInput, StringTableBuilder, TypeTableBuilder};

/// Build the type table, interning type, member, and name strings into the
/// shared string table. Threads the string table by value because it extends it.
pub fn build_type_table(
    input: &EmitInput<'_>,
    mut strings: StringTableBuilder,
) -> Result<(TypeTableBuilder, StringTableBuilder), EmitError> {
    let mut types = TypeTableBuilder::new();
    types.build(
        input.type_ctx,
        input.dependency_analysis,
        input.interner,
        &mut strings,
    )?;
    Ok((types, strings))
}
