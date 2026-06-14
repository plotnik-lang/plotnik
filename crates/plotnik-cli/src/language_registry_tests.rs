//! Corpus-level invariants for the structural skeleton that `Grammar::from_raw`
//! retains (`plotnik_core::grammar::StructureTable`).
//!
//! Every shipped grammar is built through the real `from_raw` path and its table
//! checked: it is populated, and every descent `body` points at a real variable.
//! Loading every grammar in a debug test build also exercises `classify_step`'s
//! invariant that a visible step never resolves to neither an id nor a body — a
//! resolution or alignment regression surfaces on whichever grammar trips it.

use crate::language_registry::{all, from_name};

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
