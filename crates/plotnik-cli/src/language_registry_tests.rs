//! Corpus-level invariants for the structural skeleton that `Grammar::from_raw`
//! retains (`plotnik_lib::grammar::StructureTable`).
//!
//! Every shipped grammar is built through the real `from_raw` path and its table
//! checked: it is populated, and every descent `body` points at a real variable.
//! Loading every grammar in a debug test build also exercises `classify_step`'s
//! invariant that a visible step never resolves to neither an id nor a body — a
//! resolution or alignment regression surfaces on whichever grammar trips it.

use std::collections::HashSet;

#[cfg(any(
    feature = "lang-lua",
    feature = "lang-go",
    feature = "lang-java",
    feature = "lang-rust",
    feature = "lang-python",
    feature = "lang-typescript"
))]
use indoc::indoc;

use crate::language_registry::{self, Lang, all, from_name};

#[test]
fn structure_table_resolves_across_all_grammars() {
    let mut empty = Vec::new();
    let mut dangling = Vec::new();

    for lang in all() {
        let table = lang.grammar().structure();

        if table.variables().is_empty() {
            empty.push(lang.name().to_string());
            continue;
        }

        for variable in table.variables() {
            for (p, production) in variable.productions.iter().enumerate() {
                for (s, step) in production.iter().enumerate() {
                    if let Some(body) = step.target.body
                        && table.variable(body).is_none()
                    {
                        dangling.push(format!(
                            "{}: {}/prod[{p}]/step[{s}] -> {body:?}",
                            lang.name(),
                            variable.name
                        ));
                    }
                }
            }
        }
    }

    assert!(
        empty.is_empty(),
        "grammars with an empty structure table: {empty:?}"
    );
    assert!(
        dangling.is_empty(),
        "descent bodies pointing at missing variables: {dangling:?}"
    );
}

/// Locks the two cases the four-way model used to mishandle: an aliased
/// non-terminal must keep a descent `body`, and an inlined supertype must be
/// reachable by `body` despite having no public id. Skips gracefully when a
/// language is not compiled in.
#[test]
fn descends_through_aliases_and_inlined_supertypes() {
    // TypeScript `interface_body` is an alias of `object_type` (a non-terminal).
    // Recording only the alias id would strand its productions; the body must
    // descend into a variable that has them.
    if let Some(ts) = from_name("typescript") {
        let ts = ts.grammar();
        let interface_body = ts
            .resolve_named_node("interface_body")
            .expect("typescript exposes the interface_body kind");
        let mut seen = false;
        for variable in ts.structure().variables() {
            for production in &variable.productions {
                for step in production {
                    if step.target.id == Some(interface_body) {
                        seen = true;
                        let body = step
                            .target
                            .body
                            .expect("aliased non-terminal keeps a descent body");
                        assert!(
                            !ts.structure()
                                .variable(body)
                                .unwrap()
                                .productions
                                .is_empty(),
                            "interface_body descends into a variable with productions"
                        );
                    }
                }
            }
        }
        assert!(seen, "expected interface_body to appear as a step");
    }

    // Go `_type` is a supertype tree-sitter inlines: no public id, but a `_type`
    // position must still be reachable by descent so it can expand to the concrete
    // types.
    if let Some(go) = from_name("go") {
        let go = go.grammar();
        assert!(
            go.resolve_named_node("_type").is_none(),
            "_type is inlined, so it has no public id"
        );
        let mut seen = false;
        for variable in go.structure().variables() {
            for production in &variable.productions {
                for step in production {
                    if let Some(body) = step.target.body
                        && go
                            .structure()
                            .variable(body)
                            .is_some_and(|v| v.name == "_type")
                    {
                        seen = true;
                        assert_eq!(step.target.id, None, "a `_type` reference carries no id");
                    }
                }
            }
        }
        assert!(
            seen,
            "expected a step descending into go's `_type` variable"
        );
    }
}

#[derive(Debug)]
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct NodeKey {
    type_name: String,
    named: bool,
}

fn compare_lang(lang: &Lang) -> CheckResult {
    let production = lang.grammar();
    let reference = lang.ts_language();
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
        .map(u16::from);
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
            production.resolve_field(name).map(u16::from),
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
                production: production.resolve_named_node(name).map(u16::from),
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
                production: production.resolve_anonymous_node(name).map(u16::from),
                reference: None,
            });
        }
    }

    for name in production.all_field_names() {
        if !seen_fields.contains(name) {
            differences.push(Difference::Field {
                name: name.to_string(),
                production: production.resolve_field(name).map(u16::from),
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

#[test]
fn abi_compat_all_languages() {
    let langs = language_registry::all();
    let expected = language_registry::enabled_language_names();
    if expected.is_empty() {
        #[cfg(feature = "all-languages")]
        panic!("no languages registered");
    }

    let registered = langs.iter().map(|lang| lang.name()).collect::<HashSet<_>>();
    assert_eq!(
        registered.len(),
        expected.len(),
        "enabled language feature count must match registered languages"
    );
    for name in expected {
        assert!(
            registered.contains(name),
            "enabled language `{name}` was not registered"
        );
    }

    for lang in langs {
        let result = compare_lang(lang);
        if let CheckResult::Mismatch { differences } = &result {
            let details: Vec<String> = differences.iter().map(format_difference).collect();
            panic!(
                "ABI mismatch for '{}':\n  {}",
                lang.name(),
                details.join("\n  ")
            );
        }
    }
}

/// Soundness regression for grammar verification: the grammar must admit every
/// `(parent, field, value)` and `(parent, child)` that a real parse actually produces —
/// the "never reject the possible" half of the contract. Each snippet exercises a
/// transparency path the node-shape summary alone drops:
///
/// - lua `chunk.local_declaration` — a field applied to a *supertype member* (`declaration`)
///   that surfaces on the enclosing node;
/// - go `var_spec.type: (slice_type)` — a field value typed by the *inlined* supertype `_type`;
/// - java `field_declaration.type: (integral_type)` — a value reached through a *kept*
///   supertype (`_unannotated_type`) whose own member is an inlined sub-supertype;
/// - python `module → match_statement` — a child reached through a hidden `_statement` chain.
///
/// Asserting against the real tree is stronger than comparing the two grammar views, since the
/// parser is the ground truth for what can appear.
#[test]
#[cfg(any(
    feature = "lang-lua",
    feature = "lang-go",
    feature = "lang-java",
    feature = "lang-rust",
    feature = "lang-python",
    feature = "lang-typescript"
))]
fn grammar_admits_every_field_and_child_in_real_trees() {
    const SNIPPETS: &[(&str, &str)] = &[
        (
            "lua",
            indoc! {"
                local function f() end
                local x = 1
            "},
        ),
        (
            "go",
            indoc! {"
                package p
                var x []int
                var y map[string]int
                func g() T { return a }
            "},
        ),
        ("java", "class A { int x = 1; void m() { return; } }"),
        ("rust", "fn f() -> T { let x: U = a; }"),
        (
            "python",
            indoc! {"
                match x:
                    case 1:
                        pass
            "},
        ),
        ("typescript", "function f(a: number): void { return; }"),
    ];

    let mut checked = 0usize;
    let mut violations = Vec::new();
    for (lang_name, source) in SNIPPETS {
        let Some(lang) = from_name(lang_name) else {
            continue;
        };
        let tree = lang.parse_source(source);
        assert!(
            !tree.root_node().has_error(),
            "{lang_name} snippet must parse cleanly for the regression to be meaningful"
        );
        check_real_tree_admissibility(
            lang.grammar(),
            tree.root_node(),
            lang_name,
            &mut checked,
            &mut violations,
        );
    }

    assert!(
        checked > 0,
        "regression checked nothing — no bug languages compiled in?"
    );
    assert!(
        violations.is_empty(),
        "grammar rejects a field/child that appears in a real parse tree:\n  {}",
        violations.join("\n  ")
    );
}

/// Walk `node`'s real children, asserting the grammar admits each one where it appears:
/// a fielded child against its field, a bare named child against its parent. Mirrors the
/// admissibility the linker applies (`check.rs`), expanding each declared supertype to its
/// concrete members via `collect_subtypes` before comparing.
#[cfg(any(
    feature = "lang-lua",
    feature = "lang-go",
    feature = "lang-java",
    feature = "lang-rust",
    feature = "lang-python",
    feature = "lang-typescript"
))]
fn check_real_tree_admissibility(
    grammar: &plotnik_lib::grammar::Grammar,
    node: tree_sitter::Node<'_>,
    lang: &str,
    checked: &mut usize,
    violations: &mut Vec<String>,
) {
    // `declared` lists kinds, possibly supertypes; `value` is the concrete kind in the real
    // tree. Admissible iff it is listed, or a member of a listed supertype.
    let admits = |declared: &[_], value| {
        declared.contains(&value)
            || declared
                .iter()
                .any(|&seed| grammar.collect_subtypes(seed).contains(&value))
    };

    let parent_kind = node.kind();
    let parent_id = grammar.resolve_named_node(parent_kind);
    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return;
    }

    loop {
        let child = cursor.node();
        if child.is_named()
            && let Some(parent) = parent_id
            && let Some(value) = grammar.resolve_named_node(child.kind())
        {
            *checked += 1;
            match cursor.field_name() {
                Some(field_name) => {
                    if let Some(field) = grammar.resolve_field(field_name) {
                        if !grammar.has_field(parent, field) {
                            violations.push(format!(
                                "[{lang}] `{parent_kind}` lacks field `{field_name}` (real value `{}`)",
                                child.kind()
                            ));
                        } else if !admits(grammar.valid_field_types(parent, field), value) {
                            violations.push(format!(
                                "[{lang}] `{parent_kind}.{field_name}` rejects real value `{}`",
                                child.kind()
                            ));
                        }
                    }
                }
                None => {
                    if !admits(grammar.valid_child_types(parent), value) && !grammar.is_extra(value)
                    {
                        violations.push(format!(
                            "[{lang}] `{parent_kind}` rejects real child `{}`",
                            child.kind()
                        ));
                    }
                }
            }
        }
        check_real_tree_admissibility(grammar, child, lang, checked, violations);
        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Names the exact field/value pairs the three supertype mechanisms used to reject, each
/// recoverable only because field admissibility now treats supertypes and hidden rules as
/// transparent (see `core::grammar::types::Reachability`):
///
/// - lua: `local_declaration` is a field on a member of the `declaration` supertype, so it
///   surfaces on the enclosing `chunk`;
/// - go: `var_spec.type` is typed by the inlined supertype `_type`, whose concrete members
///   the node-shape summary drops;
/// - java: `field_declaration.type` is the kept supertype `_unannotated_type`, which reaches
///   `integral_type` only through an inlined sub-supertype (the `collect_subtypes` splice).
#[test]
#[cfg(any(feature = "lang-lua", feature = "lang-go", feature = "lang-java"))]
fn admits_field_values_reached_through_supertypes() {
    let cases = [
        ("lua", "chunk", "local_declaration", "function_declaration"),
        ("lua", "chunk", "local_declaration", "variable_declaration"),
        ("go", "var_spec", "type", "slice_type"),
        ("go", "var_spec", "type", "map_type"),
        ("java", "field_declaration", "type", "integral_type"),
    ];
    let mut checked = 0usize;
    for (lang_name, parent, field, value) in cases {
        let Some(lang) = from_name(lang_name) else {
            continue;
        };
        checked += 1;
        let grammar = lang.grammar();
        let parent_id = grammar
            .resolve_named_node(parent)
            .unwrap_or_else(|| panic!("{lang_name}: no node kind `{parent}`"));
        let field_id = grammar
            .resolve_field(field)
            .unwrap_or_else(|| panic!("{lang_name}: no field `{field}`"));
        let value_id = grammar
            .resolve_named_node(value)
            .unwrap_or_else(|| panic!("{lang_name}: no node kind `{value}`"));

        assert!(
            grammar.has_field(parent_id, field_id),
            "{lang_name}: `{parent}` must have field `{field}`"
        );
        let declared = grammar.valid_field_types(parent_id, field_id);
        let admissible = declared.contains(&value_id)
            || declared
                .iter()
                .any(|&seed| grammar.collect_subtypes(seed).contains(&value_id));
        assert!(
            admissible,
            "{lang_name}: `{parent}.{field}` must admit `{value}`"
        );
    }
    assert!(
        checked > 0,
        "regression checked nothing — no bug languages compiled in?"
    );
}
