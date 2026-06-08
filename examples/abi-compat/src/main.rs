use std::collections::HashSet;
use std::num::NonZeroU16;
use std::process::ExitCode;

use plotnik::language_registry::{self, Lang};

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool, String> {
    let langs = selected_langs()?;
    let mut ok = true;

    for lang in langs {
        ok &= check_lang(lang);
    }

    Ok(ok)
}

fn selected_langs() -> Result<Vec<&'static Lang>, String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return Ok(language_registry::all());
    }

    let mut langs = Vec::new();
    let mut seen = HashSet::new();
    for arg in args {
        if arg == "-h" || arg == "--help" {
            return Err(usage());
        }
        if arg.starts_with('-') {
            return Err(format!("unknown argument: {arg}\n\n{}", usage()));
        }

        let Some(lang) = language_registry::from_name(&arg) else {
            return Err(format!("unknown language: {arg}\n\n{}", usage()));
        };
        if seen.insert(lang.name()) {
            langs.push(lang);
        }
    }

    Ok(langs)
}

fn check_lang(lang: &Lang) -> bool {
    match compare_lang(lang) {
        CheckResult::Match => {
            println!("{} [OK]", display_name(lang));
            true
        }
        CheckResult::Mismatch { differences } => {
            println!("{} [FAIL]", display_name(lang));
            for difference in differences {
                println!("  {}", format_difference(&difference));
            }
            false
        }
    }
}

fn display_name(lang: &Lang) -> String {
    let raw_name = &lang.raw().name;
    if raw_name == lang.name() {
        lang.name().to_string()
    } else {
        format!("{} [{}]", lang.name(), raw_name)
    }
}

enum CheckResult {
    Match,
    Mismatch { differences: Vec<Difference> },
}

#[derive(Debug)]
enum Difference {
    Node {
        key: NodeKey,
        production: Option<u16>,
        reference: Option<u16>,
    },
    Field {
        name: String,
        production: Option<u16>,
        reference: Option<u16>,
    },
}

fn compare_lang(lang: &Lang) -> CheckResult {
    let production = lang.grammar();
    let reference = lang.language();
    let mut differences = Vec::new();
    let mut seen_nodes = HashSet::new();
    let mut seen_fields = HashSet::new();

    for id in 1..reference.node_kind_count() {
        let id = u16::try_from(id).expect("tree-sitter node kind IDs fit in u16");
        let Some(name) = reference.node_kind_for_id(id) else {
            continue;
        };
        let supertype = reference.node_kind_is_supertype(id);
        if !reference.node_kind_is_visible(id) && !supertype {
            continue;
        }

        let named = reference.node_kind_is_named(id) || supertype;
        let key = NodeKey {
            type_name: name.to_string(),
            named,
        };
        if !seen_nodes.insert(key.clone()) {
            continue;
        }

        let production_id = if named {
            production.resolve_named_node(name)
        } else {
            production.resolve_anonymous_node(name)
        }
        .map(NonZeroU16::get);
        let reference_id = non_zero_id(reference.id_for_node_kind(name, named));

        push_node_difference(&mut differences, key, production_id, reference_id);
    }

    for id in 1..=reference.field_count() {
        let id = u16::try_from(id).expect("tree-sitter field IDs fit in u16");
        let Some(name) = reference.field_name_for_id(id) else {
            continue;
        };
        seen_fields.insert(name.to_string());

        push_field_difference(
            &mut differences,
            name.to_string(),
            production.resolve_field(name).map(NonZeroU16::get),
            Some(id),
        );
    }

    for name in production.all_named_node_kinds() {
        let key = NodeKey {
            type_name: name.to_string(),
            named: true,
        };
        if !seen_nodes.contains(&key) {
            differences.push(Difference::Node {
                key,
                production: production.resolve_named_node(name).map(NonZeroU16::get),
                reference: None,
            });
        }
    }

    for name in production.all_anonymous_node_kinds() {
        let key = NodeKey {
            type_name: name.to_string(),
            named: false,
        };
        if !seen_nodes.contains(&key) {
            differences.push(Difference::Node {
                key,
                production: production.resolve_anonymous_node(name).map(NonZeroU16::get),
                reference: None,
            });
        }
    }

    for name in production.all_field_names() {
        if !seen_fields.contains(name) {
            differences.push(Difference::Field {
                name: name.to_string(),
                production: production.resolve_field(name).map(NonZeroU16::get),
                reference: None,
            });
        }
    }

    if differences.is_empty() {
        CheckResult::Match
    } else {
        CheckResult::Mismatch { differences }
    }
}

fn non_zero_id(id: u16) -> Option<u16> {
    (id != 0).then_some(id)
}

fn push_node_difference(
    differences: &mut Vec<Difference>,
    key: NodeKey,
    production: Option<u16>,
    reference: Option<u16>,
) {
    if production == reference {
        return;
    }

    differences.push(Difference::Node {
        key,
        production,
        reference,
    });
}

fn push_field_difference(
    differences: &mut Vec<Difference>,
    name: String,
    production: Option<u16>,
    reference: Option<u16>,
) {
    if production == reference {
        return;
    }

    differences.push(Difference::Field {
        name,
        production,
        reference,
    });
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct NodeKey {
    type_name: String,
    named: bool,
}

impl std::fmt::Display for NodeKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.named {
            write!(f, "({})", self.type_name)
        } else {
            write!(f, "{:?}", self.type_name)
        }
    }
}

fn format_difference(difference: &Difference) -> String {
    match difference {
        Difference::Node {
            key,
            production,
            reference,
        } => format!(
            "{key} {} != {}",
            format_id(*production),
            format_id(*reference)
        ),
        Difference::Field {
            name,
            production,
            reference,
        } => format!(
            "{name}: {} != {}",
            format_id(*production),
            format_id(*reference)
        ),
    }
}

fn format_id(id: Option<u16>) -> String {
    id.map(|id| id.to_string())
        .unwrap_or_else(|| "<missing>".to_string())
}

fn usage() -> String {
    "usage: cargo run --manifest-path examples/abi-compat/Cargo.toml -- [LANG ...]\n\nWith no LANG arguments, checks every language in the shared Plotnik CLI registry.".to_string()
}
