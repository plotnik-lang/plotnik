use super::Grammar;
use super::prepared::VariableType;
use super::raw::RawGrammar;
use super::structure::{AdmissibilityStep, FieldValueProjection, SkeletonStep, StepTarget};
use indoc::indoc;

#[test]
fn distills_resolved_productions() {
    let json = indoc! {r#"{
            "name": "test",
            "rules": {
                "source_file": {
                    "type": "SEQ",
                    "members": [
                        { "type": "ALIAS", "value": "type_name", "named": true,
                          "content": { "type": "SYMBOL", "name": "identifier" } },
                        { "type": "STRING", "value": "fn" },
                        { "type": "FIELD", "name": "name",
                          "content": { "type": "SYMBOL", "name": "identifier" } },
                        { "type": "ALIAS", "value": "block_alias", "named": true,
                          "content": { "type": "SYMBOL", "name": "block" } },
                        { "type": "SYMBOL", "name": "_body" },
                        { "type": "FIELD", "name": "body",
                          "content": { "type": "SYMBOL", "name": "_body" } }
                    ]
                },
                "identifier": { "type": "PATTERN", "value": "[a-z]+" },
                "_body": { "type": "SYMBOL", "name": "block" },
                "block": {
                    "type": "SEQ",
                    "members": [
                        { "type": "STRING", "value": "{" },
                        { "type": "STRING", "value": "}" }
                    ]
                }
            }
        }"#};
    let raw = RawGrammar::from_json(json).unwrap();
    let grammar = Grammar::from_raw(&raw).unwrap();

    let table = grammar.structure();
    let find = |name: &str| {
        table
            .variables()
            .iter()
            .find(|v| v.name == name)
            .unwrap_or_else(|| panic!("variable {name:?} missing from structure table"))
    };

    let source = find("source_file");
    assert_eq!(source.id, grammar.resolve_named_node("source_file"));
    assert!(source.id.is_some());

    let steps = &source.productions[0];
    assert_eq!(steps.len(), 6);

    // [0] alias of a TOKEN (identifier -> type_name): has a public id, nothing to
    // descend into.
    assert_eq!(steps[0].target.id, grammar.resolve_named_node("type_name"));
    assert_eq!(steps[0].target.body, None);

    // [1] anonymous token "fn": id only, no field.
    assert_eq!(steps[1].target.id, grammar.resolve_anonymous_node("fn"));
    assert_eq!(steps[1].target.body, None);
    assert_eq!(steps[1].field, None);

    // [2] named token bound to field `name`: id + field, no descent.
    assert_eq!(steps[2].target.id, grammar.resolve_named_node("identifier"));
    assert_eq!(steps[2].target.body, None);
    assert_eq!(steps[2].field, grammar.resolve_field("name"));
    assert!(steps[2].field.is_some());

    // [3] alias of a NON-TERMINAL (block -> block_alias): keeps BOTH its public id
    // and a descent link into the underlying variable. This is the case that used
    // to lose its body.
    assert_eq!(
        steps[3].target.id,
        grammar.resolve_named_node("block_alias")
    );
    let aliased_body = steps[3]
        .target
        .body
        .expect("aliased non-terminal keeps a descent body");
    assert_eq!(table.variable(aliased_body).unwrap().name, "block");

    // [4] hidden non-terminal `_body`: no public id, descends into its variable.
    assert_eq!(steps[4].target.id, None);
    let inner = steps[4]
        .target
        .body
        .expect("hidden non-terminal descends into its variable");
    assert_eq!(table.variable(inner).unwrap().name, "_body");

    // [5] fielded hidden non-terminal: no value id of its own, so field
    // admissibility descends to the value frontier.
    assert_eq!(steps[5].target.id, None);
    let fielded_inner = steps[5]
        .target
        .body
        .expect("fielded hidden non-terminal descends into its variable");
    assert_eq!(fielded_inner, inner);
    let body_field = grammar.resolve_field("body").unwrap();
    assert_eq!(steps[5].field, Some(body_field));

    // `block` is a visible named non-terminal: it has an id of its own.
    let block = find("block");
    assert_eq!(block.id, grammar.resolve_named_node("block"));
    assert!(block.id.is_some());
    assert_eq!(block.kind, VariableType::Named);

    // `_body` is hidden — no id — and its production splices to `block`.
    let body_var = find("_body");
    assert_eq!(body_var.kind, VariableType::Hidden);
    assert_eq!(body_var.id, None);
    let spliced = body_var.productions[0][0]
        .target
        .body
        .expect("descends into block");
    assert_eq!(table.variable(spliced).unwrap().name, "block");

    let identifier = grammar.resolve_named_node("identifier").unwrap();
    let name_field = grammar.resolve_field("name").unwrap();
    assert_eq!(
        steps[2].admissibility(&grammar),
        AdmissibilityStep::Field {
            field: name_field,
            value: FieldValueProjection::Kind(identifier),
        }
    );

    assert_eq!(
        steps[5].admissibility(&grammar),
        AdmissibilityStep::Field {
            field: body_field,
            value: FieldValueProjection::Frontier(fielded_inner),
        }
    );

    let hidden_leaf = SkeletonStep {
        target: StepTarget {
            id: None,
            body: None,
        },
        field: None,
    };
    assert_eq!(
        hidden_leaf.admissibility(&grammar),
        AdmissibilityStep::HiddenLeaf
    );
    assert_eq!(hidden_leaf.field_value(), FieldValueProjection::Empty);
}
