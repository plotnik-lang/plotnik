//! Tests for CLI dispatch logic.
//!
//! These tests verify:
//! 1. Unified flags: dump/exec/trace accept each other's flags without error
//! 2. Help visibility: hidden flags don't appear in --help
//! 3. Positional shifting: -q shifts first positional to source
//! 4. Params extraction: correct fields are extracted from ArgMatches

use std::path::PathBuf;

use super::*;
use crate::cli::commands::{
    ast_command, check_command, dump_command, exec_command, infer_command, trace_command,
};

#[test]
fn dump_accepts_trace_flags() {
    let cmd = dump_command();
    let result = cmd.try_get_matches_from(["dump", "query.ptk", "--fuel", "500", "-vv"]);
    assert!(
        result.is_ok(),
        "dump should accept trace flags: {:?}",
        result.err()
    );

    let m = result.unwrap();
    let params = DumpParams::from_matches(&m);

    // Query path is extracted
    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    // fuel and verbose are parsed but not in DumpParams (that's the point)
}

#[test]
fn dump_accepts_exec_flags() {
    let cmd = dump_command();
    let result = cmd.try_get_matches_from([
        "dump",
        "query.ptk",
        "--compact",
        "--check",
        "--verbose-nodes",
        "--entry",
        "Foo",
    ]);
    assert!(
        result.is_ok(),
        "dump should accept exec flags: {:?}",
        result.err()
    );

    let m = result.unwrap();
    let params = DumpParams::from_matches(&m);
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
    let params = DumpParams::from_matches(&m);
    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    // source_path is parsed but not in DumpParams
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
fn exec_accepts_trace_flags() {
    let cmd = exec_command();
    let result = cmd.try_get_matches_from([
        "exec",
        "query.ptk",
        "app.js",
        "--fuel",
        "500",
        "-vv",
        "--no-result",
    ]);
    assert!(
        result.is_ok(),
        "exec should accept trace flags: {:?}",
        result.err()
    );

    let m = result.unwrap();
    let params = ExecParams::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
    // fuel, verbose, no_result are parsed but not in ExecParams
}

#[test]
fn trace_accepts_exec_flags() {
    let cmd = trace_command();
    let result = cmd.try_get_matches_from([
        "trace",
        "query.ptk",
        "app.js",
        "--compact",
        "--verbose-nodes",
        "--check",
    ]);
    assert!(
        result.is_ok(),
        "trace should accept exec flags: {:?}",
        result.err()
    );

    let m = result.unwrap();
    let params = TraceParams::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
    // compact, verbose_nodes, check are parsed but not in TraceParams
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
fn check_accepts_exec_flags() {
    let cmd = check_command();
    let result = cmd.try_get_matches_from([
        "check",
        "query.ptk",
        "--compact",
        "--verbose-nodes",
        "--check",
        "--entry",
        "Foo",
    ]);
    assert!(
        result.is_ok(),
        "check should accept exec flags: {:?}",
        result.err()
    );
}

#[test]
fn check_accepts_trace_flags() {
    let cmd = check_command();
    let result =
        cmd.try_get_matches_from(["check", "query.ptk", "--fuel", "500", "-vv", "--no-result"]);
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
fn infer_accepts_exec_flags() {
    let cmd = infer_command();
    let result = cmd.try_get_matches_from([
        "infer",
        "query.ptk",
        "--compact",
        "--check",
        "--entry",
        "Foo",
    ]);
    assert!(
        result.is_ok(),
        "infer should accept exec flags: {:?}",
        result.err()
    );
}

#[test]
fn infer_accepts_trace_flags() {
    let cmd = infer_command();
    let result =
        cmd.try_get_matches_from(["infer", "query.ptk", "--fuel", "500", "-vv", "--no-result"]);
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

    assert!(!help.contains("--fuel"), "dump help should not show --fuel");
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
fn dump_help_hides_exec_flags() {
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
        !help.contains("--check"),
        "dump help should not show --check"
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

    // SOURCE positional should be hidden
    assert!(
        !help.contains("[SOURCE]"),
        "dump help should not show SOURCE positional"
    );
    // -s/--source flag should be hidden
    assert!(
        !help.contains("Inline source text"),
        "dump help should not show -s description"
    );
}

#[test]
fn exec_help_hides_trace_flags() {
    let mut cmd = exec_command();
    let help = cmd.render_help().to_string();

    assert!(!help.contains("--fuel"), "exec help should not show --fuel");
    assert!(
        !help.contains("--no-result"),
        "exec help should not show --no-result"
    );
}

#[test]
fn trace_help_hides_exec_output_flags() {
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
    assert!(
        !help.contains("Validate output"),
        "trace help should not show --check"
    );
}

#[test]
fn check_help_hides_unified_flags() {
    let mut cmd = check_command();
    let help = cmd.render_help().to_string();

    // Source args should be hidden
    assert!(
        !help.contains("[SOURCE]"),
        "check help should not show SOURCE"
    );
    assert!(
        !help.contains("Inline source text"),
        "check help should not show -s"
    );

    // Exec flags should be hidden
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

    // Trace flags should be hidden
    assert!(
        !help.contains("--fuel"),
        "check help should not show --fuel"
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

    // Source args should be hidden
    assert!(
        !help.contains("[SOURCE]"),
        "infer help should not show SOURCE"
    );
    assert!(
        !help.contains("Inline source text"),
        "infer help should not show -s"
    );

    // Exec flags (except --verbose-nodes which is visible) should be hidden
    assert!(
        !help.contains("--compact"),
        "infer help should not show --compact"
    );
    assert!(
        !help.contains("--entry"),
        "infer help should not show --entry"
    );

    // Trace flags should be hidden
    assert!(
        !help.contains("--fuel"),
        "infer help should not show --fuel"
    );
    assert!(
        !help.contains("--no-result"),
        "infer help should not show --no-result"
    );

    // --verbose-nodes SHOULD be visible for infer
    assert!(
        help.contains("--verbose-nodes"),
        "infer help SHOULD show --verbose-nodes"
    );
}

#[test]
fn exec_shifts_positional_with_inline_query() {
    let cmd = exec_command();
    let result = cmd.try_get_matches_from(["exec", "-q", "(identifier) @id", "app.js"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = ExecParams::from_matches(&m);

    // With -q, the single positional should become source_path, not query_path
    assert_eq!(params.query_path, None);
    assert_eq!(params.query_text, Some("(identifier) @id".to_string()));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn exec_no_shift_with_both_positionals() {
    let cmd = exec_command();
    let result = cmd.try_get_matches_from(["exec", "query.ptk", "app.js"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = ExecParams::from_matches(&m);

    // Without -q, both positionals are used as-is
    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn trace_shifts_positional_with_inline_query() {
    let cmd = trace_command();
    let result = cmd.try_get_matches_from(["trace", "-q", "(identifier) @id", "app.js"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = TraceParams::from_matches(&m);

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
        "--fuel",
        "500",
        "--color",
        "always",
    ]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = TraceParams::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
    assert_eq!(params.lang, Some("typescript".to_string()));
    assert_eq!(params.entry, Some("Main".to_string()));
    assert_eq!(params.verbose, 2);
    assert!(params.no_result);
    assert_eq!(params.fuel, 500);
    assert!(matches!(params.color, ColorChoice::Always));
}

#[test]
fn exec_params_extracts_all_fields() {
    let cmd = exec_command();
    let result = cmd.try_get_matches_from([
        "exec",
        "query.ptk",
        "app.js",
        "-l",
        "javascript",
        "--compact",
        "--entry",
        "Query",
        "--color",
        "never",
        // These are parsed but not extracted (visible but unimplemented flags)
        "--verbose-nodes",
        "--check",
    ]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = ExecParams::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
    assert_eq!(params.lang, Some("javascript".to_string()));
    assert!(params.compact);
    assert_eq!(params.entry, Some("Query".to_string()));
    assert!(matches!(params.color, ColorChoice::Never));
    // verbose_nodes and check are parsed but not in ExecParams (unimplemented)
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
        // All these are accepted but ignored
        "app.rs",
        "--fuel",
        "100",
        "--compact",
    ]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = DumpParams::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.lang, Some("rust".to_string()));
    assert!(matches!(params.color, ColorChoice::Auto));
    // No source_path, fuel, compact fields in DumpParams
}

// AST command tests

#[test]
fn ast_accepts_exec_flags() {
    let cmd = ast_command();
    let result = cmd.try_get_matches_from([
        "ast",
        "query.ptk",
        "app.js",
        "--compact",
        "--check",
        "--verbose-nodes",
        "--entry",
        "Foo",
    ]);
    assert!(
        result.is_ok(),
        "ast should accept exec flags: {:?}",
        result.err()
    );

    let m = result.unwrap();
    let params = AstParams::from_matches(&m);
    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn ast_accepts_trace_flags() {
    let cmd = ast_command();
    let result = cmd.try_get_matches_from([
        "ast",
        "query.ptk",
        "app.js",
        "--fuel",
        "500",
        "-vv",
        "--no-result",
    ]);
    assert!(
        result.is_ok(),
        "ast should accept trace flags: {:?}",
        result.err()
    );

    let m = result.unwrap();
    let params = AstParams::from_matches(&m);
    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn ast_shifts_positional_with_inline_query() {
    let cmd = ast_command();
    let result = cmd.try_get_matches_from(["ast", "-q", "(identifier) @id", "app.js"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = AstParams::from_matches(&m);

    // With -q, the single positional should become source_path, not query_path
    assert_eq!(params.query_path, None);
    assert_eq!(params.query_text, Some("(identifier) @id".to_string()));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn ast_no_shift_with_both_positionals() {
    let cmd = ast_command();
    let result = cmd.try_get_matches_from(["ast", "query.ptk", "app.js"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = AstParams::from_matches(&m);

    // Without -q, both positionals are used as-is
    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn ast_help_shows_raw_flag() {
    let mut cmd = ast_command();
    let help = cmd.render_help().to_string();

    assert!(help.contains("--raw"), "ast help should show --raw");
}

#[test]
fn ast_help_hides_unified_flags() {
    let mut cmd = ast_command();
    let help = cmd.render_help().to_string();

    // Exec flags should be hidden
    assert!(
        !help.contains("--compact"),
        "ast help should not show --compact"
    );
    assert!(
        !help.contains("--verbose-nodes"),
        "ast help should not show --verbose-nodes"
    );
    assert!(
        !help.contains("--check"),
        "ast help should not show --check"
    );
    assert!(
        !help.contains("--entry"),
        "ast help should not show --entry"
    );

    // Trace flags should be hidden
    assert!(!help.contains("--fuel"), "ast help should not show --fuel");
    assert!(
        !help.contains("--no-result"),
        "ast help should not show --no-result"
    );
    assert!(
        !help.contains("Verbosity level"),
        "ast help should not show -v description"
    );
}

#[test]
fn ast_params_extracts_all_fields() {
    let cmd = ast_command();
    let result = cmd.try_get_matches_from([
        "ast",
        "query.ptk",
        "app.js",
        "-l",
        "typescript",
        "--raw",
        "--color",
        "always",
    ]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = AstParams::from_matches(&m);

    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
    assert_eq!(params.lang, Some("typescript".to_string()));
    assert!(params.raw);
    assert!(matches!(params.color, ColorChoice::Always));
}

// Test that other commands accept --raw (unified flag)

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
fn exec_accepts_raw_flag() {
    let cmd = exec_command();
    let result = cmd.try_get_matches_from(["exec", "query.ptk", "app.js", "--raw"]);
    assert!(
        result.is_ok(),
        "exec should accept --raw flag: {:?}",
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

// Extension-based detection tests for ast command

#[test]
fn ast_detects_ptk_as_query() {
    let cmd = ast_command();
    let result = cmd.try_get_matches_from(["ast", "query.ptk"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = AstParams::from_matches(&m);

    // .ptk extension → treat as query
    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, None);
}

#[test]
fn ast_detects_non_ptk_as_source() {
    let cmd = ast_command();
    let result = cmd.try_get_matches_from(["ast", "app.js"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = AstParams::from_matches(&m);

    // Non-.ptk extension → treat as source
    assert_eq!(params.query_path, None);
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn ast_no_extension_detection_with_two_positionals() {
    let cmd = ast_command();
    let result = cmd.try_get_matches_from(["ast", "query.ptk", "app.js"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = AstParams::from_matches(&m);

    // Two positionals → first is query, second is source (no detection)
    assert_eq!(params.query_path, Some(PathBuf::from("query.ptk")));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}

#[test]
fn ast_no_extension_detection_with_inline_query() {
    let cmd = ast_command();
    let result = cmd.try_get_matches_from(["ast", "-q", "(id) @x", "app.js"]);
    assert!(result.is_ok());

    let m = result.unwrap();
    let params = AstParams::from_matches(&m);

    // -q provided → positional shift takes precedence, no extension detection
    assert_eq!(params.query_path, None);
    assert_eq!(params.query_text, Some("(id) @x".to_string()));
    assert_eq!(params.source_path, Some(PathBuf::from("app.js")));
}
