//! Hidden compatibility helpers for manual grammar ABI checks.

use std::fmt;

use super::raw::RawGrammar;
use super::tree_sitter::{self, FieldSymbol, GrammarMetadata, NodeSymbol};

const MAX_DIFFERENCES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataCompatibility {
    pub result: MetadataCompatibilityResult,
}

impl MetadataCompatibility {
    pub fn is_match(&self) -> bool {
        matches!(self.result, MetadataCompatibilityResult::Match { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataCompatibilityResult {
    Match {
        node_symbols: usize,
        fields: usize,
    },
    Mismatch {
        node_symbols: ComparisonCounts,
        fields: ComparisonCounts,
        differences: Vec<MetadataDifference>,
    },
    Error {
        metadata_only: Option<String>,
        full: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComparisonCounts {
    pub metadata_only: usize,
    pub full: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataDifference {
    pub section: MetadataSection,
    pub index: usize,
    pub metadata_only: Option<String>,
    pub full: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataSection {
    NodeSymbol,
    Field,
}

impl fmt::Display for MetadataSection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NodeSymbol => f.write_str("node"),
            Self::Field => f.write_str("field"),
        }
    }
}

pub fn compare_metadata_lowering(raw: &RawGrammar) -> MetadataCompatibility {
    let metadata_only = tree_sitter::metadata_for_raw(raw);
    let full = tree_sitter::full_metadata_for_raw(raw);

    match (metadata_only, full) {
        (Ok(metadata_only), Ok(full)) => compare_metadata(metadata_only, full),
        (metadata_only, full) => MetadataCompatibility {
            result: MetadataCompatibilityResult::Error {
                metadata_only: metadata_only.err(),
                full: full.err(),
            },
        },
    }
}

fn compare_metadata(
    metadata_only: GrammarMetadata,
    full: GrammarMetadata,
) -> MetadataCompatibility {
    let metadata_only_nodes = metadata_only
        .symbols
        .iter()
        .map(NodeSnapshot::from)
        .collect::<Vec<_>>();
    let full_nodes = full
        .symbols
        .iter()
        .map(NodeSnapshot::from)
        .collect::<Vec<_>>();
    let metadata_only_fields = metadata_only
        .fields
        .iter()
        .map(FieldSnapshot::from)
        .collect::<Vec<_>>();
    let full_fields = full
        .fields
        .iter()
        .map(FieldSnapshot::from)
        .collect::<Vec<_>>();

    if metadata_only_nodes == full_nodes && metadata_only_fields == full_fields {
        return MetadataCompatibility {
            result: MetadataCompatibilityResult::Match {
                node_symbols: metadata_only_nodes.len(),
                fields: metadata_only_fields.len(),
            },
        };
    }

    let mut differences = Vec::new();
    collect_differences(
        MetadataSection::NodeSymbol,
        &metadata_only_nodes,
        &full_nodes,
        &mut differences,
    );
    collect_differences(
        MetadataSection::Field,
        &metadata_only_fields,
        &full_fields,
        &mut differences,
    );

    MetadataCompatibility {
        result: MetadataCompatibilityResult::Mismatch {
            node_symbols: ComparisonCounts {
                metadata_only: metadata_only_nodes.len(),
                full: full_nodes.len(),
            },
            fields: ComparisonCounts {
                metadata_only: metadata_only_fields.len(),
                full: full_fields.len(),
            },
            differences,
        },
    }
}

fn collect_differences<T: Eq + fmt::Display>(
    section: MetadataSection,
    metadata_only: &[T],
    full: &[T],
    differences: &mut Vec<MetadataDifference>,
) {
    let len = metadata_only.len().max(full.len());
    for index in 0..len {
        if differences.len() >= MAX_DIFFERENCES {
            return;
        }

        let metadata_only_entry = metadata_only.get(index);
        let full_entry = full.get(index);
        if metadata_only_entry == full_entry {
            continue;
        }

        differences.push(MetadataDifference {
            section,
            index,
            metadata_only: metadata_only_entry.map(ToString::to_string),
            full: full_entry.map(ToString::to_string),
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NodeSnapshot {
    id: u16,
    type_name: String,
    named: bool,
    visible: bool,
    supertype: bool,
}

impl From<&NodeSymbol> for NodeSnapshot {
    fn from(symbol: &NodeSymbol) -> Self {
        Self {
            id: symbol.id,
            type_name: symbol.type_name.clone(),
            named: symbol.named,
            visible: symbol.visible,
            supertype: symbol.supertype,
        }
    }
}

impl fmt::Display for NodeSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "id={} type={} named={} visible={} supertype={}",
            self.id, self.type_name, self.named, self.visible, self.supertype
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FieldSnapshot {
    id: u16,
    name: String,
}

impl From<&FieldSymbol> for FieldSnapshot {
    fn from(field: &FieldSymbol) -> Self {
        Self {
            id: field.id,
            name: field.name.clone(),
        }
    }
}

impl fmt::Display for FieldSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "id={} name={}", self.id, self.name)
    }
}
