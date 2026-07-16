use indoc::indoc;

use super::format::{FormatError, format_query, format_query_measured, format_query_with_config};
use crate::compiler::diagnostics::{DiagnosticKind, Error};
use crate::compiler::parse::ParseConfig;

#[test]
fn formats_inline_spacing_and_normalizations() {
    let input = "Q=(expression/binary_expression left:(identifier==\"x\")@id)::Ignored";

    let output = format_query(input);

    assert!(matches!(output, Err(FormatError::Parse { .. })));
    assert_eq!(
        format_query("Q=(identifier=='x')@id::Name").expect("query formats"),
        "Q = (identifier == \"x\") @id :: Name\n"
    );
    assert_eq!(
        format_query("Q=(identifier==/foo/)").expect("parse-clean mismatch formats"),
        "Q = (identifier == /foo/)\n"
    );
}

#[test]
fn formats_empty_input_to_one_newline() {
    assert_eq!(format_query(" \t\r\n").expect("empty query formats"), "\n");
}

#[test]
fn shebang_preserves_authored_trailing_spaces() {
    let input = "#!/usr/bin/env plotnik  \r\nQ = (a)";

    let output = format_query(input).expect("query with shebang formats");

    assert_eq!(output, "#!/usr/bin/env plotnik  \nQ = (a)\n");
}

#[test]
fn leading_comments_do_not_create_definition_blank_lines() {
    let input = "A = (a)\n// B docs\nB = (b)";

    let output = format_query(input).expect("query formats");

    assert_eq!(output, "A = (a)\n// B docs\nB = (b)\n");
}

#[test]
fn breaks_structural_lists() {
    let input = "Q = (binary_expression left: (_) @left right: (_) @right)";

    let output = format_query(input).expect("query formats");

    assert_eq!(
        output,
        indoc! {"
            Q = (binary_expression
              left: (_) @left
              right: (_) @right
            )
        "}
    );
}

#[test]
fn keeps_single_child_groups_inline_inside_broken_parent() {
    let input = "Q = [(identifier) (call (Expr))]";

    let output = format_query(input).expect("query formats");

    assert_eq!(output, "Q = [\n  (identifier)\n  (call (Expr))\n]\n");
}

#[test]
fn semantic_landmark_budget_breaks_dense_paths() {
    let cases = [
        ("Q = (foo (bar (baz)))", "Q = (foo (bar (baz)))\n"),
        (
            "Q = (foo (bar (baz (qux))))",
            indoc! {"
                Q = (foo
                  (bar (baz (qux)))
                )
            "},
        ),
        (
            "Q = (foo (bar (baz (qux (quux)))))",
            indoc! {"
                Q = (foo
                  (bar
                    (baz (qux (quux)))
                  )
                )
            "},
        ),
    ];

    for (input, expected) in cases {
        let output = format_query(input).expect("landmark path formats");

        assert_eq!(output, expected);
        assert_eq!(format_query(&output).expect("output formats"), output);
    }
}

#[test]
fn semantic_landmarks_include_navigation_predicates_and_modifiers() {
    let inline = [
        "Q = (pair key: (identifier))",
        "Q = (identifier == \"x\") @id",
        "Q = {(identifier) @id}",
        "Q = [(identifier) @id]",
        "Q = (array . (identifier))",
        "Q = (identifier) @id :: Name",
    ];
    for input in inline {
        assert_eq!(
            format_query(input).expect("three-landmark query formats"),
            format!("{input}\n")
        );
    }

    let broken = [
        "Q = (root (pair key: (identifier)))",
        "Q = (program (identifier == \"x\") @id)",
        "Q = (program (identifier)* @ids)",
        "Q = (array . (identifier) @id)",
        "Q = (program (identifier) @id :: Name)",
    ];
    for input in broken {
        let output = format_query(input).expect("four-landmark query formats");

        assert!(output.lines().count() > 1, "{input}: {output}");
        assert_eq!(format_query(&output).expect("output formats"), output);
    }
}

#[test]
fn semantic_landmarks_count_contextual_alternative_prefixes() {
    assert_eq!(
        format_query("Q = [Only: (wrapper (identifier))]").expect("labeled alternation formats"),
        indoc! {"
            Q = [
              Only: (wrapper (identifier))
            ]
        "}
    );
    assert_eq!(
        format_query("Q = [Only: (outer (wrapper (identifier)))]")
            .expect("dense labeled alternation formats"),
        indoc! {"
            Q = [
              Only: (outer
                (wrapper (identifier))
              )
            ]
        "}
    );
}

#[test]
fn semantic_landmarks_ignore_token_length_and_category_refinement() {
    let long_kind = "a".repeat(100);
    let long = format!("Q = ({long_kind})");

    assert_eq!(
        format_query(&long).expect("long atom formats"),
        format!("{long}\n")
    );
    assert_eq!(
        format_query("Q = (expression#binary_expression)").expect("refinement formats"),
        "Q = (expression#binary_expression)\n"
    );
}

#[test]
fn breaks_capture_dense_groups() {
    let input = "Q = {(identifier) @id} @inner";

    let output = format_query(input).expect("query formats");

    assert_eq!(
        output,
        indoc! {"
            Q = {
              (identifier) @id
            } @inner
        "}
    );
}

#[test]
fn output_is_idempotent() {
    let input = "A=[One:(a)@a Two:(b)@b]\nB=(program (expression_statement (id)@id))";
    let once = format_query(input).expect("query formats");

    let twice = format_query(&once).expect("formatted query reparses");

    assert_eq!(twice, once);
}

#[test]
fn nested_capture_breaks_reach_a_fixed_point() {
    let input = "Q = (program {(expression_statement (identifier) @_inner)} @_outer (debugger_statement) @d)";

    let once = format_query(input).expect("query formats");
    let twice = format_query(&once).expect("formatted query reparses");

    assert_eq!(twice, once);
    assert!(once.lines().all(|line| line.matches('@').count() <= 1));
}

#[test]
fn preserves_comments_when_a_group_breaks() {
    let input = indoc! {"
        // before
        Q = (call
          /* first */
          (a) // after a
          (b)
        )
    "};

    let output = format_query(input).expect("query formats");

    assert!(output.contains("// before"));
    assert!(output.contains("/* first */"));
    assert!(output.contains("// after a"));
    assert_eq!(output.matches("// before").count(), 1);
    assert_eq!(output.matches("/* first */").count(), 1);
    assert_eq!(output.matches("// after a").count(), 1);
    assert_eq!(format_query(&output).expect("output formats"), output);
}

#[test]
fn line_comment_is_a_breakable_unit_in_an_empty_group() {
    let input = "Q = (program // intentionally empty\n)";

    let output = format_query(input).expect("parse-clean commented group formats");

    assert_eq!(output, "Q = (program // intentionally empty\n)\n");
    assert_eq!(format_query(&output).expect("output formats"), output);
}

#[test]
fn preserves_comments_owned_by_pattern_wrappers() {
    let cases = [
        "Q = foo: /* field */ (call (a) (b))",
        "Q = (call (a) (b)) /* capture */ @x",
        "Q = (call (a) (b)) /* quantifier */ *",
        "Q /* separator */ = (call (a) (b))",
        "Q = [Label /* alternative */ : (call (a) (b))]",
        "Q = (Foo /* ref */)",
        "Q = (identifier /* predicate */ == \"x\")",
        "Q = (identifier) @x :: /* capture type */ Name",
        "Q = (call (a) (b) /* closer */)",
        "Q = (call (a) (b)) /* multiline\nblock */ @x",
    ];

    for input in cases {
        let output = format_query(input).expect("commented wrapper formats");
        assert_eq!(comment_texts(&output), comment_texts(input), "{input}");
        assert_eq!(format_query(&output).expect("output formats"), output);
    }
}

#[test]
fn root_trailing_comment_stays_with_its_definition() {
    let input = "A = (a) // tail\nB = (b)";

    let output = format_query(input).expect("query formats");

    assert_eq!(output, "A = (a) // tail\nB = (b)\n");
}

#[test]
fn root_inline_comment_stays_in_the_definition_gap() {
    let input = "A = (a) /* gap */ B = (b)";

    let output = format_query(input).expect("query formats");

    assert_eq!(output, "A = (a) /* gap */\nB = (b)\n");
}

#[test]
fn preserves_identical_comments_by_source_identity() {
    let input = "Q = (call /* same */ (a) /* same */ (b))";

    let output = format_query(input).expect("query formats");

    assert_eq!(output.matches("/* same */").count(), 2);
}

#[test]
fn wrapper_and_inner_comments_have_distinct_attachment_identity() {
    let input = "Q = (a/* inner */) /* outer */ @x";

    let output = format_query(input).expect("comments on coincident wrapper ranges format");

    assert_eq!(comment_texts(&output), vec!["/* inner */", "/* outer */"]);
    assert_eq!(format_query(&output).expect("output formats"), output);
}

#[test]
fn forcing_comment_nested_in_a_group_head_is_emitted() {
    let input = indoc! {r#"
        Q = (call *=
          /* predicate value */
          "x"
          (a)
          (b)
        )
    "#};

    let output = format_query(input).expect("nested group-head comment formats");

    assert_eq!(comment_texts(&output), vec!["/* predicate value */"]);
    assert_eq!(format_query(&output).expect("output formats"), output);
}

#[test]
fn lone_carriage_returns_in_block_comments_normalize_to_lf() {
    let input = "Q = (a /* first\rsecond */)";

    let output = format_query(input).expect("lone carriage return formats");

    assert!(!output.contains('\r'));
    assert!(output.contains("/* first\nsecond */"));
    assert_eq!(format_query(&output).expect("output formats"), output);
}

#[test]
fn forcing_comments_keep_source_order_across_suffix_boundaries() {
    let input = "Q = (a) // first\n@x\n/* second */\n:: T";

    let output = format_query(input).expect("ordered suffix comments format");

    assert_eq!(comment_texts(&output), vec!["// first", "/* second */"]);
    assert_eq!(format_query(&output).expect("output formats"), output);
}

#[test]
fn forcing_comments_never_cross_later_comments() {
    let cases = [
        "Q // first\n= /* second */ (a)",
        "Q = field: // first\n/* second */ (a)",
        "Q = (a) // first\n/* second */ @x",
        "Q = (a) // first\n/* second */ *",
    ];

    for input in cases {
        let output = format_query(input).expect("mixed boundary comments format");

        assert_eq!(comment_texts(&output), vec!["// first", "/* second */"]);
        assert_eq!(format_query(&output).expect("output formats"), output);
    }
}

#[test]
fn definition_comment_keeps_blank_line_classification_stable() {
    let input = "A // tail\n= (a)\nB = (b)";

    let output = format_query(input).expect("definition comment formats");

    assert_eq!(format_query(&output).expect("output formats"), output);
    assert_eq!(comment_texts(&output), vec!["// tail"]);
}

#[test]
fn multiline_file_trailing_comment_counts_toward_definition_spacing() {
    let input = "A = (a) /* first\nsecond */\nB = (b)";

    let output = format_query(input).expect("multiline trailing comment formats");

    assert_eq!(format_query(&output).expect("output formats"), output);
    assert!(output.contains("second */\n\nB = (b)"), "{output}");
}

#[test]
fn outer_suffix_participates_in_width_measurement() {
    let long_kind = "a".repeat(61);
    let input = format!("Q = ({long_kind} (identifier)) @long_capture_name");

    let output = format_query(&input).expect("query formats");

    assert!(output.lines().count() > 1, "{output}");
}

#[test]
fn deeply_nested_broken_groups_format_without_combinatorial_rendering() {
    let mut pattern = "(leaf)".to_owned();
    for _ in 0..48 {
        pattern = format!("(node {pattern} (sibling))");
    }
    let input = format!("Q = {pattern}");

    let output = format_query(&input).expect("deep query formats");

    assert_eq!(format_query(&output).expect("output formats"), output);
}

fn comment_texts(source: &str) -> Vec<&str> {
    crate::compiler::parse::lex(source)
        .into_iter()
        .filter(|token| {
            matches!(
                token.kind,
                crate::compiler::parse::SyntaxKind::LineComment
                    | crate::compiler::parse::SyntaxKind::BlockComment
            )
        })
        .map(|token| {
            let range = std::ops::Range::<usize>::from(token.span);
            &source[range]
        })
        .collect()
}

#[test]
fn rejects_parse_errors_and_propagates_limits() {
    assert!(matches!(
        format_query("Q = ("),
        Err(FormatError::Parse { .. })
    ));
    assert!(matches!(
        format_query_with_config(
            "Q = (identifier)",
            ParseConfig {
                fuel: 0,
                max_depth: 100,
            }
        ),
        Err(FormatError::Resource(Error::ParseFuelExhausted))
    ));
}

#[test]
fn rejects_recovered_group_without_closer() {
    let error = format_query("((\"(\"").expect_err("unclosed nested group is rejected");
    let diagnostics = error.diagnostics().expect("syntax failure has diagnostics");

    assert_eq!(diagnostics.error_count(), 1);
    assert!(
        diagnostics
            .kinds()
            .any(|kind| kind == DiagnosticKind::UnclosedTree)
    );
}

#[test]
fn parse_errors_include_their_rendering_source_map() {
    let error = format_query("Q = (").expect_err("broken query is rejected");
    let FormatError::Parse {
        diagnostics,
        source_map,
    } = error
    else {
        panic!("syntax error returns parse diagnostics")
    };

    let rendered = diagnostics.render(&source_map);

    assert!(rendered.contains("Q = ("), "{rendered}");
}

#[test]
fn adversarial_shapes_stay_within_a_linear_work_budget() {
    let shapes = [
        (
            comment_heavy_query(200),
            comment_heavy_query(400),
            "comments",
        ),
        (flat_group_query(400), flat_group_query(800), "flat group"),
        (nested_field_query(48), nested_field_query(96), "fields"),
    ];

    for (small, large, shape) in shapes {
        let (_, small_metrics) = format_query_measured(&small, ParseConfig::default())
            .expect("small adversarial query formats");
        let (_, large_metrics) = format_query_measured(&large, ParseConfig::default())
            .expect("large adversarial query formats");

        assert!(
            large_metrics.work <= small_metrics.work * 3,
            "{shape} work grew faster than input: {small_metrics:?} -> {large_metrics:?}"
        );
        assert!(
            large_metrics.work
                <= (large_metrics.input_bytes + large_metrics.output_bytes).saturating_mul(32),
            "{shape} exceeded its absolute linear budget: {large_metrics:?}"
        );
    }

    let dense = nested_landmark_query(96);
    let (_, metrics) = format_query_measured(&dense, ParseConfig::default())
        .expect("dense landmark chain formats");
    assert!(
        metrics.work <= (metrics.input_bytes + metrics.output_bytes).saturating_mul(32),
        "dense landmark chain exceeded its input-plus-output budget: {metrics:?}"
    );
}

fn comment_heavy_query(count: usize) -> String {
    let items = std::iter::repeat_n("/* comment */ (leaf)", count)
        .collect::<Vec<_>>()
        .join(" ");
    format!("Q = (root {items})")
}

fn flat_group_query(count: usize) -> String {
    let items = std::iter::repeat_n("(leaf)", count)
        .collect::<Vec<_>>()
        .join(" ");
    format!("Q = (root {items})")
}

fn nested_field_query(depth: usize) -> String {
    let mut pattern = "(leaf)".to_owned();
    for _ in 0..depth {
        pattern = format!("field: {pattern}");
    }
    format!("Q = {pattern}")
}

fn nested_landmark_query(depth: usize) -> String {
    let mut pattern = "(leaf)".to_owned();
    for _ in 0..depth {
        pattern = format!("(node {pattern})");
    }
    format!("Q = {pattern}")
}
