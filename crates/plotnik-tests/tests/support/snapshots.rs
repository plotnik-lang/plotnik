use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_EXT: &str = "txt";

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
    Recording,
    Traced { inspection: InspectionPolicy },
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
                inspection: if name.contains("inspection") || name.contains("recording") {
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
                mode: if name.contains("recording") {
                    VmMode::Recording
                } else {
                    VmMode::Traced {
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
                SectionKind::Recording,
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
    Recording,
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
            Self::Recording => "recording",
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
    SectionKind::Recording,
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

pub(crate) fn discover(root: &Path) -> Vec<Fixture> {
    let mut out = Vec::new();
    let entries = fs::read_dir(root).expect("tests/ directory must be readable");
    for entry in entries {
        let entry = entry
            .unwrap_or_else(|error| panic!("read fixture entry in {}: {error}", root.display()));
        let path = entry.path();
        if path.is_dir()
            && let Some(stage) = path.file_name().and_then(|name| name.to_str())
            && is_stage_dir(stage)
        {
            walk(&path, stage, root, &mut out);
        }
    }
    out.sort_by(|left, right| left.name.as_str().cmp(right.name.as_str()));
    out
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

fn is_stage_dir(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && bytes[2] == b'-'
        && !name.starts_with("01-")
}

fn walk(dir: &Path, stage: &str, root: &Path, out: &mut Vec<Fixture>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("read fixture dir {}: {error}", dir.display()));
    for entry in entries {
        let entry = entry
            .unwrap_or_else(|error| panic!("read fixture entry in {}: {error}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            walk(&path, stage, root, out);
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some(FIXTURE_EXT) {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .expect("fixture path is under tests root")
            .with_extension("");
        let name = FixtureName::from_relative(&relative)
            .unwrap_or_else(|error| panic!("name {}: {error}", path.display()));
        let kind = FixtureKind::classify(stage, &name)
            .unwrap_or_else(|error| panic!("classify {}: {error}", path.display()));
        out.push(Fixture { path, name, kind });
    }
}
