//! Tests for CLI dispatch logic.
//!
//! These tests verify:
//! 1. Unified flags: dump/run/trace accept each other's flags without error
//! 2. Help visibility: hidden flags don't appear in --help
//! 3. Positional shifting: -q shifts first positional to source
//! 4. Params extraction: correct fields are extracted from ArgMatches

use std::path::PathBuf;

use plotnik_lib::Limit;

use super::*;
use crate::cli::commands::{
    check_command, dump_command, generate_command, infer_command, inspect_command, run_command,
    trace_command, tree_command,
};
use crate::commands::generate::GenerateTarget;

#[test]
fn generate_extracts_rust_target_and_grammar_path() {
    let matches = generate_command()
        .try_get_matches_from([
            "generate",
            "query.ptk",
            "--target",
            "rust",
            "--grammar",
            "grammar.json",
            "-o",
            "query.rs",
        ])
        .unwrap();

    let options = GenerateOpts::from_matches(&matches);

    assert_eq!(options.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(options.target, GenerateTarget::Rust);
    assert_eq!(options.grammar, Some(PathBuf::from("grammar.json")));
    assert_eq!(options.output, Some(PathBuf::from("query.rs")));
}

#[test]
fn generate_rejects_registry_and_external_grammar_together() {
    let result = generate_command().try_get_matches_from([
        "generate",
        "query.ptk",
        "--target",
        "rust",
        "--grammar",
        "grammar.json",
        "-l",
        "javascript",
    ]);

    assert!(result.is_err());
}

#[test]
fn dump_accepts_trace_flags() {
    let cmd = dump_command();
    let result = cmd.try_get_matches_from(["dump", "query.ptk", "--max-steps", "500", "-vv"]);
    assert!(
        result.is_ok(),
        "dump should accept trace flags: {:?}",
        result.err()
    );

    let m = result.unwrap();
    let params = DumpOpts::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
}

#[test]
fn dump_accepts_run_flags() {
    let cmd = dump_command();
    let result = cmd.try_get_matches_from([
        "dump",
        "query.ptk",
        "--compact",
        "--verbose-nodes",
        "--entry",
        "Foo",
    ]);
    assert!(
        result.is_ok(),
        "dump should accept run flags: {:?}",
        result.err()
    );

    let m = result.unwrap();
    let params = DumpOpts::from_matches(&m);
    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
}

#[test]
fn dump_accepts_source_positional() {
    let cmd = dump_command();
    let result = cmd.try_get_matches_from(["dump", "query.ptk", "app.js"]);
    assert!(
        result.is_ok(),
        "dump should accept source positional: {:?}",
        result.err()
    );

    let m = result.unwrap();
    let params = DumpOpts::from_matches(&m);
    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
}

#[test]
fn dump_accepts_source_flag() {
    let cmd = dump_command();
    let result = cmd.try_get_matches_from(["dump", "query.ptk", "-s", "let x = 1"]);
    assert!(
        result.is_ok(),
        "dump should accept -s flag: {:?}",
        result.err()
    );
}

#[test]
fn run_accepts_trace_flags() {
    let cmd = run_command();
    let result = cmd.try_get_matches_from([
        "run",
        "query.ptk",
        "app.js",
        "--max-steps",
        "500",
        "-vv",
        "--no-result",
    ]);
    assert!(
        result.is_ok(),
        "run should accept trace flags: {:?}",
        result.err()
    );

    let m = result.unwrap();
    let params = RunOpts::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn trace_accepts_run_flags() {
    let cmd = trace_command();
    let result = cmd.try_get_matches_from([
        "trace",
        "query.ptk",
        "app.js",
        "--compact",
        "--verbose-nodes",
    ]);
    assert!(
        result.is_ok(),
        "trace should accept run flags: {:?}",
        result.err()
    );

    let m = result.unwrap();
    let params = TraceOpts::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn check_accepts_source_args() {
    let cmd = check_command();
    let result = cmd.try_get_matches_from(["check", "query.ptk", "app.js", "-s", "let x"]);
    assert!(
        result.is_ok(),
        "check should accept source args: {:?}",
        result.err()
    );
}

#[test]
fn check_accepts_run_flags() {
    let cmd = check_command();
    let result = cmd.try_get_matches_from([
        "check",
        "query.ptk",
        "--compact",
        "--verbose-nodes",
        "--entry",
        "Foo",
    ]);
    assert!(
        result.is_ok(),
        "check should accept run flags: {:?}",
        result.err()
    );
}

#[test]
fn check_accepts_trace_flags() {
    let cmd = check_command();
    let result = cmd.try_get_matches_from([
        "check",
        "query.ptk",
        "--max-steps",
        "500",
        "-vv",
        "--no-result",
    ]);
    assert!(
        result.is_ok(),
        "check should accept trace flags: {:?}",
        result.err()
    );
}

#[test]
fn infer_accepts_source_args() {
    let cmd = infer_command();
    let result = cmd.try_get_matches_from(["infer", "query.ptk", "app.js", "-s", "let x"]);
    assert!(
        result.is_ok(),
        "infer should accept source args: {:?}",
        result.err()
    );
}

#[test]
fn infer_accepts_run_flags() {
    let cmd = infer_command();
    let result = cmd.try_get_matches_from(["infer", "query.ptk", "--compact", "--entry", "Foo"]);
    assert!(
        result.is_ok(),
        "infer should accept run flags: {:?}",
        result.err()
    );
}

#[test]
fn infer_accepts_trace_flags() {
    let cmd = infer_command();
    let result = cmd.try_get_matches_from([
        "infer",
        "query.ptk",
        "--max-steps",
        "500",
        "-vv",
        "--no-result",
    ]);
    assert!(
        result.is_ok(),
        "infer should accept trace flags: {:?}",
        result.err()
    );
}

#[test]
fn dump_help_hides_trace_flags() {
    let mut cmd = dump_command();
    let help = cmd.render_help().to_string();

    assert!(
        !help.contains("--max-steps"),
        "dump help should not show --max-steps"
    );
    assert!(
        !help.contains("--no-result"),
        "dump help should not show --no-result"
    );
    assert!(
        !help.contains("Verbosity level"),
        "dump help should not show -v description"
    );
}

#[test]
fn dump_help_hides_run_flags() {
    let mut cmd = dump_command();
    let help = cmd.render_help().to_string();

    assert!(
        !help.contains("--compact"),
        "dump help should not show --compact"
    );
    assert!(
        !help.contains("--verbose-nodes"),
        "dump help should not show --verbose-nodes"
    );
    assert!(
        !help.contains("--entry"),
        "dump help should not show --entry"
    );
}

#[test]
fn dump_help_hides_source_args() {
    let mut cmd = dump_command();
    let help = cmd.render_help().to_string();

    assert!(
        !help.contains("[SOURCE]"),
        "dump help should not show SOURCE positional"
    );
    assert!(
        !help.contains("Inline source text"),
        "dump help should not show -s description"
    );
}

#[test]
fn run_help_hides_trace_flags() {
    let mut cmd = run_command();
    let help = cmd.render_help().to_string();

    assert!(
        !help.contains("--no-result"),
        "run help should not show --no-result"
    );
}

#[test]
fn run_help_shows_limit_flags() {
    let mut cmd = run_command();
    let help = cmd.render_help().to_string();

    assert!(
        help.contains("--max-steps"),
        "run help SHOULD show --max-steps"
    );
    assert!(
        help.contains("--max-memory"),
        "run help SHOULD show --max-memory"
    );
}

#[test]
fn trace_help_hides_run_output_flags() {
    let mut cmd = trace_command();
    let help = cmd.render_help().to_string();

    assert!(
        !help.contains("--compact"),
        "trace help should not show --compact"
    );
    assert!(
        !help.contains("--verbose-nodes"),
        "trace help should not show --verbose-nodes"
    );
}

#[test]
fn check_help_hides_unified_flags() {
    let mut cmd = check_command();
    let help = cmd.render_help().to_string();

    assert!(
        !help.contains("[SOURCE]"),
        "check help should not show SOURCE"
    );
    assert!(
        !help.contains("Inline source text"),
        "check help should not show -s"
    );
    assert!(
        !help.contains("--compact"),
        "check help should not show --compact"
    );
    assert!(
        !help.contains("--verbose-nodes"),
        "check help should not show --verbose-nodes"
    );
    assert!(
        !help.contains("--entry"),
        "check help should not show --entry"
    );
    assert!(
        !help.contains("--max-steps"),
        "check help should not show --max-steps"
    );
    assert!(
        !help.contains("--no-result"),
        "check help should not show --no-result"
    );
}

#[test]
fn infer_help_hides_unified_flags() {
    let mut cmd = infer_command();
    let help = cmd.render_help().to_string();

    assert!(
        !help.contains("[SOURCE]"),
        "infer help should not show SOURCE"
    );
    assert!(
        !help.contains("Inline source text"),
        "infer help should not show -s"
    );
    assert!(
        !help.contains("--compact"),
        "infer help should not show --compact"
    );
    assert!(
        !help.contains("--entry"),
        "infer help should not show --entry"
    );
    assert!(
        !help.contains("--max-steps"),
        "infer help should not show --max-steps"
    );
    assert!(
        !help.contains("--no-result"),
        "infer help should not show --no-result"
    );
    assert!(
        help.contains("--verbose-nodes"),
        "infer help SHOULD show --verbose-nodes"
    );
}

#[test]
fn run_shifts_positional_with_inline_query() {
    let cmd = run_command();
    let result = cmd.try_get_matches_from(["run", "-q", "(identifier) @id", "app.js"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = RunOpts::from_matches(&m);

    assert_eq!(params.query_path, None);
    assert_eq!(params.query_text, Some("(identifier) @id".to_string()));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn run_no_shift_with_both_positionals() {
    let cmd = run_command();
    let result = cmd.try_get_matches_from(["run", "query.ptk", "app.js"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = RunOpts::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn trace_shifts_positional_with_inline_query() {
    let cmd = trace_command();
    let result = cmd.try_get_matches_from(["trace", "-q", "(identifier) @id", "app.js"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = TraceOpts::from_matches(&m);

    assert_eq!(params.query_path, None);
    assert_eq!(params.query_text, Some("(identifier) @id".to_string()));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn trace_params_extracts_all_fields() {
    let cmd = trace_command();
    let result = cmd.try_get_matches_from([
        "trace",
        "query.ptk",
        "app.js",
        "-l",
        "typescript",
        "--entry",
        "Main",
        "-vv",
        "--no-result",
        "--max-steps",
        "500",
        "--color",
        "always",
    ]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = TraceOpts::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
    assert_eq!(params.lang, Some("typescript".to_string()));
    assert_eq!(params.entry, Some("Main".to_string()));
    assert_eq!(params.verbose, 2);
    assert!(params.no_result);
    assert_eq!(params.limits.steps, Limit::Of(500));
    assert!(matches!(params.color, ColorChoice::Always));
}

#[test]
fn run_params_extracts_all_fields() {
    let cmd = run_command();
    let result = cmd.try_get_matches_from([
        "run",
        "query.ptk",
        "app.js",
        "-l",
        "javascript",
        "--compact",
        "--entry",
        "Query",
        "--color",
        "never",
        "--verbose-nodes",
    ]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = RunOpts::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
    assert_eq!(params.lang, Some("javascript".to_string()));
    assert!(params.compact);
    assert_eq!(params.entry, Some("Query".to_string()));
    assert!(matches!(params.color, ColorChoice::Never));
}

#[test]
fn dump_params_extracts_only_relevant_fields() {
    let cmd = dump_command();
    let result = cmd.try_get_matches_from([
        "dump",
        "query.ptk",
        "-l",
        "rust",
        "--color",
        "auto",
        "app.rs",
        "--max-steps",
        "100",
        "--compact",
    ]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = DumpOpts::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.lang, Some("rust".to_string()));
    assert!(matches!(params.color, ColorChoice::Auto));
}

#[test]
fn tree_accepts_run_flags() {
    let cmd = tree_command();
    let result = cmd.try_get_matches_from([
        "tree",
        "query.ptk",
        "app.js",
        "--compact",
        "--verbose-nodes",
        "--entry",
        "Foo",
    ]);
    assert!(
        result.is_ok(),
        "tree should accept run flags: {:?}",
        result.err()
    );

    let m = result.unwrap();
    let params = TreeOpts::from_matches(&m);
    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn tree_accepts_trace_flags() {
    let cmd = tree_command();
    let result = cmd.try_get_matches_from([
        "tree",
        "query.ptk",
        "app.js",
        "--max-steps",
        "500",
        "-vv",
        "--no-result",
    ]);
    assert!(
        result.is_ok(),
        "tree should accept trace flags: {:?}",
        result.err()
    );

    let m = result.unwrap();
    let params = TreeOpts::from_matches(&m);
    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn tree_shifts_positional_with_inline_query() {
    let cmd = tree_command();
    let result = cmd.try_get_matches_from(["tree", "-q", "(identifier) @id", "app.js"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = TreeOpts::from_matches(&m);

    assert_eq!(params.query_path, None);
    assert_eq!(params.query_text, Some("(identifier) @id".to_string()));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn tree_no_shift_with_both_positionals() {
    let cmd = tree_command();
    let result = cmd.try_get_matches_from(["tree", "query.ptk", "app.js"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = TreeOpts::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn tree_help_shows_precise_tree_flags() {
    let mut cmd = tree_command();
    let help = cmd.render_help().to_string();

    assert!(help.contains("--query-view"));
    assert!(help.contains("--include-anonymous"));
    assert!(!help.contains("--raw"), "tree help should not show --raw");
}

#[test]
fn tree_help_shows_json_flag() {
    let mut cmd = tree_command();
    let help = cmd.render_help().to_string();

    assert!(help.contains("--json"), "tree help should show --json");
}

#[test]
fn tree_help_hides_unified_flags() {
    let mut cmd = tree_command();
    let help = cmd.render_help().to_string();

    assert!(
        !help.contains("--compact"),
        "tree help should not show --compact"
    );
    assert!(
        !help.contains("--verbose-nodes"),
        "tree help should not show --verbose-nodes"
    );
    assert!(
        !help.contains("--entry"),
        "tree help should not show --entry"
    );
    assert!(
        !help.contains("--max-steps"),
        "tree help should not show --max-steps"
    );
    assert!(
        !help.contains("--no-result"),
        "tree help should not show --no-result"
    );
    assert!(
        !help.contains("Verbosity level"),
        "tree help should not show -v description"
    );
}

#[test]
fn tree_params_extracts_all_fields() {
    let cmd = tree_command();
    let result = cmd.try_get_matches_from([
        "tree",
        "query.ptk",
        "app.js",
        "-l",
        "typescript",
        "--query-view",
        "cst",
        "--include-anonymous",
        "--json",
        "--color",
        "always",
    ]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = TreeOpts::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
    assert_eq!(params.lang, Some("typescript".to_string()));
    assert_eq!(params.query_view, crate::commands::tree::QueryView::Cst);
    assert!(params.include_anonymous);
    assert!(params.json);
    assert!(matches!(params.color, ColorChoice::Always));
}

#[test]
fn dump_accepts_raw_flag() {
    let cmd = dump_command();
    let result = cmd.try_get_matches_from(["dump", "query.ptk", "--raw"]);
    assert!(
        result.is_ok(),
        "dump should accept --raw flag: {:?}",
        result.err()
    );
}

#[test]
fn run_accepts_raw_flag() {
    let cmd = run_command();
    let result = cmd.try_get_matches_from(["run", "query.ptk", "app.js", "--raw"]);
    assert!(
        result.is_ok(),
        "run should accept --raw flag: {:?}",
        result.err()
    );
}

#[test]
fn trace_accepts_raw_flag() {
    let cmd = trace_command();
    let result = cmd.try_get_matches_from(["trace", "query.ptk", "app.js", "--raw"]);
    assert!(
        result.is_ok(),
        "trace should accept --raw flag: {:?}",
        result.err()
    );
}

#[test]
fn check_accepts_raw_flag() {
    let cmd = check_command();
    let result = cmd.try_get_matches_from(["check", "query.ptk", "--raw"]);
    assert!(
        result.is_ok(),
        "check should accept --raw flag: {:?}",
        result.err()
    );
}

#[test]
fn tree_detects_ptk_as_query() {
    let cmd = tree_command();
    let result = cmd.try_get_matches_from(["tree", "query.ptk"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = TreeOpts::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, None);
}

#[test]
fn tree_detects_non_ptk_as_source() {
    let cmd = tree_command();
    let result = cmd.try_get_matches_from(["tree", "app.js"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = TreeOpts::from_matches(&m);

    assert_eq!(params.query_path, None);
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn tree_no_extension_detection_with_two_positionals() {
    let cmd = tree_command();
    let result = cmd.try_get_matches_from(["tree", "query.ptk", "app.js"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = TreeOpts::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn tree_no_extension_detection_with_inline_query() {
    let cmd = tree_command();
    let result = cmd.try_get_matches_from(["tree", "-q", "(id) @x", "app.js"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = TreeOpts::from_matches(&m);

    assert_eq!(params.query_path, None);
    assert_eq!(params.query_text, Some("(id) @x".to_string()));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn bare_ptk_file_routes_to_run() {
    let args = vec!["plotnik".into(), "query.ptk".into(), "app.js".into()];

    let routed = crate::cli::route_default_subcommand(args);

    assert_eq!(routed[1], "run");
    assert_eq!(routed[2], "query.ptk");
}

#[test]
fn subcommand_is_not_rerouted() {
    let args = vec!["plotnik".into(), "check".into(), "query.ptk".into()];

    let routed = crate::cli::route_default_subcommand(args);

    assert_eq!(routed[1], "check");
}

#[test]
fn flags_are_not_rerouted() {
    let args = vec!["plotnik".into(), "--version".into()];

    let routed = crate::cli::route_default_subcommand(args);

    assert_eq!(routed[1], "--version");
}

#[test]
fn exec_is_a_hidden_alias_of_run() {
    let cmd = crate::cli::build_cli();
    let result = cmd.try_get_matches_from(["plotnik", "exec", "query.ptk", "app.js"]);

    let m = result.unwrap();
    assert_eq!(m.subcommand_name(), Some("run"));
}

#[test]
fn check_help_shows_json_flag() {
    let mut cmd = check_command();
    let help = cmd.render_help().to_string();

    assert!(help.contains("--json"), "check help should show --json");
}

#[test]
fn run_help_hides_json_flag() {
    let mut cmd = run_command();
    let help = cmd.render_help().to_string();

    assert!(!help.contains("--json"), "run help should hide --json");
}

#[test]
fn inspect_help_shows_json_and_verbose_flags() {
    let mut cmd = inspect_command();
    let help = cmd.render_help().to_string();

    assert!(help.contains("--json"), "inspect help should show --json");
    assert!(help.contains("-v"), "inspect help should show -v");
}

#[test]
fn inspect_params_extracts_all_fields() {
    let cmd = inspect_command();
    let result = cmd.try_get_matches_from([
        "inspect",
        "query.ptk",
        "app.js",
        "-l",
        "typescript",
        "--entry",
        "Q",
        "--json",
        "-v",
        "--max-steps",
        "500",
        "--color",
        "never",
    ]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = InspectOpts::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
    assert_eq!(params.lang, Some("typescript".to_string()));
    assert_eq!(params.entry, Some("Q".to_string()));
    assert!(params.json);
    assert_eq!(params.verbose, 1);
    assert!(matches!(params.limits.steps, Limit::Of(500)));
    assert!(matches!(params.color, ColorChoice::Never));
}

#[test]
fn flag_with_ptk_value_is_not_rerouted() {
    let args = vec!["plotnik".into(), "--query=q.ptk".into(), "app.ts".into()];

    let routed = crate::cli::route_default_subcommand(args);

    assert_eq!(routed[1], "--query=q.ptk");
}
