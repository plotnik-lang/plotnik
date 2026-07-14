use std::fs;
use std::path::PathBuf;

use plotnik_lib::GrammarIdentity;

use super::generate::{GenerateArgs, GenerateTarget, generate};
use crate::error::CliError;

#[test]
fn registry_generation_records_embedded_grammar_provenance() {
    let args = GenerateArgs {
        query_path: None,
        query_text: Some("Q = (program)".to_string()),
        lang: Some("javascript".to_string()),
        grammar: None,
        target: GenerateTarget::Rust,
        output: None,
        debug: false,
        color: false,
    };

    let output = generate(&args).expect("registry query generates");

    assert!(output.contains("// Grammar name: \"javascript\""));
    assert!(output.contains("// Grammar source: \"arborium-javascript@"));
    assert!(output.contains("const GRAMMAR_SHA256: &str = \""));
}

#[test]
fn external_generation_hashes_exact_grammar_bytes() {
    let grammar_json = br#"{
      "name": "tiny",
      "rules": {
        "program": { "type": "REPEAT", "content": { "type": "SYMBOL", "name": "identifier" } },
        "identifier": { "type": "PATTERN", "value": "[a-z]+" }
      }
    }"#;
    let path = scratch_path("tiny-grammar.json");
    fs::write(&path, grammar_json).unwrap();
    let identity =
        GrammarIdentity::from_json_bytes("tiny", grammar_json, path.display().to_string());
    let args = GenerateArgs {
        query_path: None,
        query_text: Some("Q = (program)".to_string()),
        lang: None,
        grammar: Some(path.clone()),
        target: GenerateTarget::Rust,
        output: None,
        debug: false,
        color: false,
    };

    let output = generate(&args).expect("external grammar query generates");
    fs::remove_file(&path).unwrap();

    assert!(output.contains("// Grammar name: \"tiny\""));
    assert!(output.contains(identity.sha256()));
    assert!(output.contains(&format!(
        "// Grammar source: {:?}",
        path.display().to_string()
    )));
}

#[test]
fn invalid_query_is_a_domain_no() {
    let args = GenerateArgs {
        query_path: None,
        query_text: Some("Q = (not_a_javascript_kind)".to_string()),
        lang: Some("javascript".to_string()),
        grammar: None,
        target: GenerateTarget::Rust,
        output: None,
        debug: false,
        color: false,
    };

    let error = generate(&args).expect_err("invalid query must not generate");

    assert!(matches!(error, CliError::No));
}

#[test]
fn debug_generation_emits_json_entrypoint() {
    let args = GenerateArgs {
        query_path: None,
        query_text: Some("Q = (program) @root".to_string()),
        lang: Some("javascript".to_string()),
        grammar: None,
        target: GenerateTarget::Rust,
        output: None,
        debug: true,
        color: false,
    };

    let output = generate(&args).expect("debug query generates");

    assert!(output.contains("pub fn parse_to_json("));
    assert!(output.contains("rt::debug::to_json"));
    assert!(output.contains("SerializeWithSource for Q"));
}

fn scratch_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("plotnik-{}-{name}", std::process::id()))
}
