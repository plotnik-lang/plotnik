use std::path::{Path, PathBuf};

pub(crate) struct Fixture {
    pub path: PathBuf,
    pub name: FixtureName,
    pub kind: FixtureKind,
}

#[derive(Clone)]
pub(crate) struct FixtureName {
    display: String,
    components: Vec<String>,
}

impl FixtureName {
    fn from_relative(path: &Path) -> Result<Self, String> {
        let components = path
            .iter()
            .map(|component| {
                component
                    .to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("fixture path is not UTF-8: {}", path.display()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if components.is_empty() {
            return Err("fixture path has no components".into());
        }
        Ok(Self {
            display: components.join("/"),
            components,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.display
    }

    fn contains(&self, component: &str) -> bool {
        self.components.iter().any(|part| part == component)
    }

    fn contains_path(&self, path: &[&str]) -> bool {
        self.components
            .windows(path.len())
            .any(|window| window.iter().map(String::as_str).eq(path.iter().copied()))
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum TriviaPolicy {
    Include,
    Omit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InspectionPolicy {
    Include,
    Omit,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum LintPolicy {
    Strict,
    Normal,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SerdePolicy {
    Include,
    Omit,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum MappingPolicy {
    Include,
    Omit,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum VmMode {
    StructuredTrace,
    TextTrace { inspection: InspectionPolicy },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FixtureKind {
    Parser {
        trivia: TriviaPolicy,
    },
    Analyze,
    Bytecode {
        inspection: InspectionPolicy,
        lints: LintPolicy,
    },
    Types {
        serde: SerdePolicy,
        mapping: MappingPolicy,
        lints: LintPolicy,
    },
    Matcher {
        lints: LintPolicy,
    },
    Vm {
        mode: VmMode,
        lints: LintPolicy,
    },
}

impl FixtureKind {
    fn classify(stage: &str, name: &FixtureName) -> Result<Self, String> {
        let lints = if name.contains("lints") {
            LintPolicy::Strict
        } else {
            LintPolicy::Normal
        };
        match stage.split('-').next().unwrap_or("") {
            "02" => Ok(Self::Parser {
                trivia: if name.contains("trivia") {
                    TriviaPolicy::Include
                } else {
                    TriviaPolicy::Omit
                },
            }),
            "03" => Ok(Self::Analyze),
            "04" if name.contains("bytecode") => Ok(Self::Bytecode {
                inspection: if name.contains("inspection") || name.contains("execution_trace") {
                    InspectionPolicy::Include
                } else {
                    InspectionPolicy::Omit
                },
                lints,
            }),
            "04" if name.contains("types") => Ok(Self::Types {
                serde: if name.contains("serde") {
                    SerdePolicy::Include
                } else {
                    SerdePolicy::Omit
                },
                mapping: if name.contains("mapped") {
                    MappingPolicy::Include
                } else {
                    MappingPolicy::Omit
                },
                lints,
            }),
            "04" if name.contains_path(&["rust", "module"]) => Ok(Self::Matcher { lints }),
            "06" => Ok(Self::Vm {
                mode: if name.contains("execution_trace") {
                    VmMode::StructuredTrace
                } else {
                    VmMode::TextTrace {
                        inspection: if name.contains("inspection") {
                            InspectionPolicy::Include
                        } else {
                            InspectionPolicy::Omit
                        },
                    }
                },
                lints,
            }),
            _ => Err(format!(
                "unknown fixture kind `{}` under `{stage}`",
                name.as_str()
            )),
        }
    }

    pub fn preserves_query_layout(self) -> bool {
        matches!(self, Self::Parser { .. })
    }

    pub fn strict_lints(self) -> bool {
        matches!(
            self,
            Self::Bytecode {
                lints: LintPolicy::Strict,
                ..
            } | Self::Types {
                lints: LintPolicy::Strict,
                ..
            } | Self::Matcher {
                lints: LintPolicy::Strict
            } | Self::Vm {
                lints: LintPolicy::Strict,
                ..
            }
        )
    }

    pub fn legal_sections(self) -> &'static [SectionKind] {
        match self {
            Self::Parser { .. } => &[SectionKind::Diagnostics, SectionKind::Cst, SectionKind::Ast],
            Self::Analyze => &[SectionKind::Diagnostics, SectionKind::Symbols],
            Self::Bytecode { .. } => &[
                SectionKind::Diagnostics,
                SectionKind::Nfa,
                SectionKind::Bytecode,
            ],
            Self::Types { .. } => &[
                SectionKind::Diagnostics,
                SectionKind::TypeScript,
                SectionKind::Rust,
                SectionKind::Mapped,
            ],
            Self::Matcher { .. } => &[SectionKind::Diagnostics, SectionKind::Matcher],
            Self::Vm { .. } => &[
                SectionKind::TypeScript,
                SectionKind::Diagnostics,
                SectionKind::Output,
                SectionKind::Inspection,
                SectionKind::ExecutionTrace,
                SectionKind::Bytecode,
                SectionKind::Trace,
            ],
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FixtureMode {
    Check,
    AcceptAll,
}

impl FixtureMode {
    pub fn from_env() -> Result<Self, String> {
        if env_switch("SHOT")? {
            Ok(Self::AcceptAll)
        } else {
            Ok(Self::Check)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SectionKind {
    Diagnostics,
    Cst,
    Ast,
    Symbols,
    Nfa,
    Bytecode,
    TypeScript,
    Rust,
    Mapped,
    Matcher,
    Output,
    Inspection,
    ExecutionTrace,
    Trace,
}

impl SectionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Diagnostics => "diagnostics",
            Self::Cst => "cst",
            Self::Ast => "ast",
            Self::Symbols => "symbols",
            Self::Nfa => "nfa",
            Self::Bytecode => "bytecode",
            Self::TypeScript => "typescript",
            Self::Rust => "rust",
            Self::Mapped => "mapped",
            Self::Matcher => "matcher",
            Self::Output => "output",
            Self::Inspection => "inspection",
            Self::ExecutionTrace => "execution_trace",
            Self::Trace => "trace",
        }
    }

    pub fn from_header(header: &str) -> Option<Self> {
        ALL_SECTION_KINDS
            .iter()
            .copied()
            .find(|kind| kind.as_str() == header)
    }
}

const ALL_SECTION_KINDS: &[SectionKind] = &[
    SectionKind::Diagnostics,
    SectionKind::Cst,
    SectionKind::Ast,
    SectionKind::Symbols,
    SectionKind::Nfa,
    SectionKind::Bytecode,
    SectionKind::TypeScript,
    SectionKind::Rust,
    SectionKind::Mapped,
    SectionKind::Matcher,
    SectionKind::Output,
    SectionKind::Inspection,
    SectionKind::ExecutionTrace,
    SectionKind::Trace,
];

pub(crate) struct GeneratedSection {
    pub kind: SectionKind,
    pub body: String,
}

impl GeneratedSection {
    pub fn new(kind: SectionKind, body: impl Into<String>) -> Self {
        Self {
            kind,
            body: body.into(),
        }
    }
}

pub(crate) struct GeneratedOutput {
    sections: Vec<GeneratedSection>,
}

impl GeneratedOutput {
    pub fn validate(kind: FixtureKind, sections: Vec<GeneratedSection>) -> Result<Self, String> {
        let legal = kind.legal_sections();
        let mut output: Vec<GeneratedSection> = Vec::with_capacity(sections.len());
        let mut last_position = None;
        for section in sections {
            let position = legal
                .iter()
                .position(|candidate| *candidate == section.kind)
                .ok_or_else(|| format!("generated illegal `{}` section", section.kind.as_str()))?;
            if let Some(existing) = output.iter_mut().find(|item| item.kind == section.kind) {
                if section.kind != SectionKind::Diagnostics {
                    return Err(format!(
                        "generated duplicate `{}` section",
                        section.kind.as_str()
                    ));
                }
                if !existing.body.ends_with('\n') {
                    existing.body.push('\n');
                }
                existing
                    .body
                    .push_str(section.body.trim_start_matches('\n'));
                continue;
            }
            if last_position.is_some_and(|last| position < last) {
                return Err(format!(
                    "generated `{}` section out of order",
                    section.kind.as_str()
                ));
            }
            last_position = Some(position);
            output.push(section);
        }
        Ok(Self { sections: output })
    }

    pub fn into_sections(self) -> Vec<GeneratedSection> {
        self.sections
    }
}

pub(crate) fn fixture(root: &Path, relative: &str) -> Result<Fixture, String> {
    let relative_path = Path::new(relative);
    let stage = relative_path
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .ok_or_else(|| format!("fixture path has no UTF-8 stage: {relative}"))?;
    let name_path = relative_path.with_extension("");
    let name = FixtureName::from_relative(&name_path)?;
    let kind = FixtureKind::classify(stage, &name)?;
    Ok(Fixture {
        path: root.join(relative_path),
        name,
        kind,
    })
}

fn env_switch(name: &str) -> Result<bool, String> {
    match std::env::var(name) {
        Err(std::env::VarError::NotPresent) => Ok(false),
        Err(error) => Err(format!("read `{name}`: {error}")),
        Ok(value) if matches!(value.as_str(), "1" | "true") => Ok(true),
        Ok(value) if matches!(value.as_str(), "0" | "false") => Ok(false),
        Ok(value) => Err(format!(
            "`{name}` must be one of 0, 1, false, or true; got `{value}`"
        )),
    }
}
