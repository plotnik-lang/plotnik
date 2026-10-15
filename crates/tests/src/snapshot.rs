use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use plotnik_compiler::{
    InputKind, SourceCollection, SourceId, SourceMap, SourcePath, parse_input_kind,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub query: String,
    pub sections: Vec<Section>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub body: String,
}

pub fn parse_document(raw: &str) -> Result<Document, String> {
    let normalized = raw.replace("\r\n", "\n");
    let mut query_lines = Vec::new();
    let mut sections: Vec<(String, Vec<&str>)> = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;

    for line in normalized.lines() {
        if let Some(name) = parse_section_header(line) {
            if let Some(previous) = current.take() {
                sections.push(previous);
            }
            current = Some((name, Vec::new()));
            continue;
        }
        if let Some((_, body)) = current.as_mut() {
            body.push(line);
            continue;
        }
        query_lines.push(line);
    }
    if let Some(previous) = current {
        sections.push(previous);
    }

    let query = query_lines.join("\n");
    let sections = sections
        .into_iter()
        .map(|(name, body)| Section {
            name,
            body: body.join("\n"),
        })
        .collect();
    Ok(Document { query, sections })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    JavaScript,
    TypeScript,
    Dart,
}

impl Language {
    fn from_filename(filename: Option<&str>) -> Result<Self, String> {
        match filename
            .map(Path::new)
            .and_then(Path::extension)
            .and_then(|ext| ext.to_str())
        {
            None if filename.is_none() => Ok(Self::JavaScript),
            Some("js") => Ok(Self::JavaScript),
            Some("ts") => Ok(Self::TypeScript),
            Some("dart") => Ok(Self::Dart),
            _ => Err(format!(
                "unsupported input filename `{}`",
                filename.expect("named input")
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Fixture {
    pub query: String,
    pub kind: Option<InputKind>,
    pub inputs: Vec<Section>,
    pub generated: Vec<Section>,
}

impl Fixture {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let document = parse_document(raw)?;
        let kind = if document.query.is_empty() {
            None
        } else {
            let storage = SourceMap::from_inline(&document.query);
            Some(
                parse_input_kind(storage.source(Default::default()))
                    .map_err(|error| error.to_string())?,
            )
        };
        let mut inputs = Vec::new();
        let mut generated = Vec::new();
        let mut names = HashSet::new();
        if kind == Some(InputKind::Module) {
            names.insert(PathBuf::from("query.ptk"));
        }
        let mut has_program = false;
        let mut has_grammar = false;
        let mut has_module = kind == Some(InputKind::Module);
        for section in document.sections {
            let authored = section.name == "grammar"
                || section.name == "input"
                || section.filename().is_some();
            if !authored {
                generated.push(section);
                continue;
            }

            if !generated.is_empty() {
                return Err("authored inputs must precede generated sections".into());
            }

            if section.name == "grammar" {
                if has_grammar || has_program {
                    return Err("GRAMMAR requires one grammar and no programming input".into());
                }
                has_grammar = true;
            } else {
                if let Some(filename) = section.filename() {
                    if filename.is_empty()
                        || !Path::new(filename)
                            .components()
                            .all(|part| matches!(part, Component::Normal(_)))
                    {
                        return Err(format!(
                            "input filename must be a relative path: `{filename}`"
                        ));
                    }

                    if !names.insert(PathBuf::from(filename)) {
                        return Err(format!("duplicate input filename `{filename}`"));
                    }
                }
                if section.is_module() {
                    has_module = true;
                } else {
                    Language::from_filename(section.filename())?;
                    if has_program || has_grammar {
                        return Err("fixtures require at most one programming input and no GRAMMAR alongside it".into());
                    }
                    has_program = true;
                }
            }
            inputs.push(section);
        }
        if !has_module && kind != Some(InputKind::Expression) {
            return Err("fixture has no query input".into());
        }

        Ok(Self {
            query: document.query,
            kind,
            inputs,
            generated,
        })
    }

    pub fn modules(&self) -> impl Iterator<Item = (&str, &str)> {
        let opening =
            (self.kind == Some(InputKind::Module)).then_some(("query.ptk", self.query.as_str()));
        opening
            .into_iter()
            .chain(
                self.inputs
                    .iter()
                    .filter(|input| input.is_module())
                    .map(|input| {
                        (
                            input.filename().expect("module input has a filename"),
                            input.body.as_str(),
                        )
                    }),
            )
    }

    pub fn program(&self) -> Option<&Section> {
        self.inputs
            .iter()
            .find(|input| input.name != "grammar" && !input.is_module())
    }

    pub fn grammar(&self) -> Option<&str> {
        self.inputs
            .iter()
            .find(|input| input.name == "grammar")
            .map(|input| input.body.as_str())
    }

    pub fn language(&self) -> Language {
        Language::from_filename(self.program().and_then(Section::filename))
            .expect("validated programming input")
    }

    pub fn collection(&self) -> (SourceCollection, Option<SourceId>) {
        let mut storage = SourceMap::new();
        let modules: Vec<_> = self
            .modules()
            .map(|(path, body)| storage.add_file(SourcePath::new(path), body))
            .collect();
        let expression =
            (self.kind == Some(InputKind::Expression)).then(|| storage.add_stdin(&self.query));
        let mut collection = SourceCollection::new(storage);
        for id in modules {
            collection.include_directory(id);
        }
        (collection, expression)
    }
}

impl Section {
    pub fn filename(&self) -> Option<&str> {
        self.name.strip_prefix("input.")
    }

    pub fn is_module(&self) -> bool {
        self.filename().is_some_and(|filename| {
            Path::new(filename)
                .extension()
                .is_some_and(|ext| ext == "ptk")
        })
    }
}

pub fn parse_section_header(line: &str) -> Option<String> {
    let label = rule_label(line)?;
    let name = match label.split_once('(') {
        Some((head, ext)) if head.trim().eq_ignore_ascii_case("input") => {
            format!("input.{}", ext.strip_suffix(')')?.trim())
        }
        Some(_) => return None,
        None => label.to_ascii_lowercase(),
    };
    let known = name == "input"
        || name.starts_with("input.")
        || matches!(
            name.as_str(),
            "grammar"
                | "cst"
                | "ast"
                | "definitions"
                | "nfa"
                | "bytecode"
                | "mapped"
                | "typescript"
                | "rust"
                | "matcher"
                | "trace"
                | "output"
                | "inspection"
                | "execution_trace"
                | "diagnostics"
        );
    known.then_some(name)
}

fn rule_label(line: &str) -> Option<&str> {
    let line = line.trim_end();
    if !line.starts_with('-') || !line.ends_with('-') {
        return None;
    }
    let label = line
        .trim_matches('-')
        .strip_prefix(' ')?
        .strip_suffix(' ')?
        .trim();
    (!label.is_empty()).then_some(label)
}
