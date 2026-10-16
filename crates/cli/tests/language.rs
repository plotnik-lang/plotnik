use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const GRAMMAR: &str = r#"{"name":"test_lang","rules":{"program":{"type":"REPEAT","content":{"type":"SYMBOL","name":"identifier"}},"identifier":{"type":"PATTERN","value":"[a-z]+"}}}"#;
const QUERY: &str = "// @test_lang\nQ = (program (identifier) @name :: text)";

fn cache(root: &Path, entry: &str) {
    let dir = root.join(entry);
    fs::create_dir_all(&dir).expect("create cache fixture");
    fs::write(dir.join("grammar.json"), GRAMMAR).expect("write grammar fixture");
    fs::write(dir.join("parser.wasm"), b"not wasm").expect("write invalid parser fixture");
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plotnik"))
        .args(args)
        .args(["--color", "never", "--cache-dir"])
        .arg(root)
        .output()
        .expect("run plotnik")
}

fn stdin(root: &Path, args: &[&str], text: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_plotnik"))
        .args(args)
        .args(["--color", "never", "--cache-dir"])
        .arg(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run plotnik with stdin");

    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(text.as_bytes())
        .expect("write PTK module to stdin");

    child.wait_with_output().expect("wait for plotnik")
}

#[test]
fn individual_selection_uses_loaded_name_and_metadata_only() {
    let root = tempfile::tempdir().unwrap();
    cache(root.path(), "test_lang");
    cache(root.path(), "owner%2Frepo@main");
    let file = root.path().join("query.ptk");
    fs::write(&file, QUERY).unwrap();

    for command in ["check", "dump", "gen"] {
        let mut args = vec![command, file.to_str().unwrap(), "--lang", "test_lang"];
        if command == "gen" {
            args.extend(["--target", "rust"]);
        }
        let named = run(root.path(), &args);
        args[3] = "owner/repo@main";
        let explicit = run(root.path(), &args);

        assert_eq!(named.status.code(), Some(0), "{named:?}");
        assert_eq!(explicit.status.code(), Some(0), "{explicit:?}");
        let named = String::from_utf8(named.stdout).unwrap();
        let explicit = String::from_utf8(explicit.stdout).unwrap();
        let explicit = explicit.replace(
            root.path()
                .join("owner%2Frepo@main/grammar.json")
                .to_str()
                .unwrap(),
            root.path().join("test_lang/grammar.json").to_str().unwrap(),
        );
        assert_eq!(named, explicit);
        if command == "check" {
            assert_eq!(named, "");
        }
    }

    fs::write(&file, "// @test_lang\nQ = (unknown)").unwrap();
    let unknown_node = run(
        root.path(),
        &["check", file.to_str().unwrap(), "--lang", "test_lang"],
    );

    assert_eq!(unknown_node.status.code(), Some(1), "{unknown_node:?}");
    assert!(
        String::from_utf8(unknown_node.stderr)
            .unwrap()
            .contains("`unknown` is not a valid node kind")
    );
    assert_eq!(
        fs::read(root.path().join("test_lang/grammar.json")).unwrap(),
        GRAMMAR.as_bytes()
    );
    assert_eq!(
        fs::read(root.path().join("test_lang/parser.wasm")).unwrap(),
        b"not wasm"
    );
    assert_eq!(
        fs::read_dir(root.path().join("test_lang")).unwrap().count(),
        2
    );
}

#[test]
fn directories_compile_one_namespace_with_explicit_language() {
    let root = tempfile::tempdir().unwrap();
    cache(root.path(), "test_lang");
    let queries = root.path().join("queries");
    fs::create_dir(&queries).unwrap();
    fs::write(queries.join("a.ptk"), "Name = (identifier) @text :: text").unwrap();
    fs::write(queries.join("foreign.ptk"), "// @other\nnot a valid module").unwrap();
    fs::write(
        queries.join("z.ptk"),
        "// @test_lang\nQ = (program (Name) @name)",
    )
    .unwrap();
    let path = queries.to_str().unwrap();

    for command in ["check", "dump", "gen"] {
        let mut args = vec![command, path, "--lang", "test_lang"];
        if command == "gen" {
            args.extend(["--target", "rust"]);
        }
        let merged = run(root.path(), &args);
        assert_eq!(merged.status.code(), Some(0), "{merged:?}");

        fs::write(queries.join("bad.ptk"), "// @bad/name\nBad = (program)").unwrap();
        let malformed = run(root.path(), &args);
        fs::remove_file(queries.join("bad.ptk")).unwrap();

        assert_eq!(malformed.status.code(), Some(1), "{malformed:?}");
        assert!(malformed.stdout.is_empty(), "{malformed:?}");
        assert!(
            String::from_utf8(malformed.stderr)
                .unwrap()
                .contains("bad.ptk:1:5")
        );
    }
}

#[test]
fn declaration_errors_are_located_and_missing_selection_is_usage_error() {
    let root = tempfile::tempdir().unwrap();
    cache(root.path(), "test_lang");
    let file = root.path().join("query.ptk");
    fs::write(&file, "// @bad/name\nQ = (program)").unwrap();

    for command in ["check", "dump", "gen"] {
        let mut args = vec![command, file.to_str().unwrap()];
        if command == "gen" {
            args.extend(["--target", "rust"]);
        }
        let missing = run(root.path(), &args);
        args.extend(["--lang", "test_lang"]);
        let malformed = run(root.path(), &args);

        assert_eq!(missing.status.code(), Some(2), "{missing:?}");
        assert_eq!(malformed.status.code(), Some(1), "{malformed:?}");
        assert!(malformed.stdout.is_empty(), "{malformed:?}");
        let stderr = String::from_utf8(malformed.stderr).unwrap();
        assert!(stderr.contains("query.ptk:1:5"), "{stderr}");
        assert!(stderr.contains("invalid grammar declaration"), "{stderr}");
    }
}

#[test]
fn static_stdin_is_a_named_module() {
    let root = tempfile::tempdir().unwrap();
    cache(root.path(), "test_lang");

    for command in ["check", "dump", "gen"] {
        let mut args = vec![command, "-", "--lang", "test_lang"];
        if command == "gen" {
            args.extend(["--target", "rust"]);
        }
        let module = stdin(
            root.path(),
            &args,
            "Q = (program (Name) @name)\nName = (identifier) @text :: text",
        );
        let expression = stdin(root.path(), &args, "(program)");
        let malformed = stdin(root.path(), &args, "// @bad/name\nQ = (program)");

        assert_eq!(module.status.code(), Some(0), "{module:?}");
        if command == "check" {
            assert!(module.stdout.is_empty(), "{module:?}");
        }
        assert_eq!(expression.status.code(), Some(1), "{expression:?}");
        assert_eq!(malformed.status.code(), Some(1), "{malformed:?}");
        let stderr = String::from_utf8(malformed.stderr).unwrap();
        assert!(
            stderr.contains("1 | // @bad/name\n  |     ^^^^^^^^"),
            "{stderr}"
        );
    }
}

#[test]
fn compiler_warnings_preserve_success_and_output_unless_check_is_strict() {
    let root = tempfile::tempdir().unwrap();
    cache(root.path(), "test_lang");
    let query = "Q = (program [A: (identifier) B: (identifier)])";
    let expected = concat!(
        "warning: alternative labels have no output effect here: captures from the alternatives merge into the enclosing result\n",
        "  |\n",
        "1 | Q = (program [A: (identifier) B: (identifier)])\n",
        "  |              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^\n",
        "  |\n",
        "help: capture the alternation (`[...] @name`) to make its labels produce variant cases, or remove them\n",
    );

    for command in ["check", "dump", "gen"] {
        let mut args = vec![command, "-", "--lang", "test_lang"];
        if command == "gen" {
            args.extend(["--target", "rust"]);
        }
        let normal = stdin(root.path(), &args, query);
        let unlabeled = stdin(
            root.path(),
            &args,
            "Q = (program [(identifier) (identifier)])",
        );

        assert_eq!(normal.status.code(), Some(0), "{normal:?}");
        assert_eq!(String::from_utf8(normal.stderr).unwrap(), expected);
        assert_eq!(unlabeled.status.code(), Some(0), "{unlabeled:?}");
        assert!(unlabeled.stderr.is_empty(), "{unlabeled:?}");
        assert_eq!(normal.stdout, unlabeled.stdout);
        assert_eq!(normal.stdout.is_empty(), command == "check");

        if command == "gen" {
            let output = root.path().join("matcher.rs");
            args.extend(["--output", output.to_str().unwrap()]);
            let saved = stdin(root.path(), &args, query);

            assert_eq!(saved.status.code(), Some(0), "{saved:?}");
            assert!(saved.stdout.is_empty(), "{saved:?}");
            assert_eq!(fs::read(&output).unwrap(), normal.stdout);
            assert_eq!(
                String::from_utf8(saved.stderr).unwrap(),
                format!("{expected}Wrote Rust matcher to {}\n", output.display())
            );
        }
    }

    let strict = stdin(
        root.path(),
        &["check", "-", "--lang", "test_lang", "--strict"],
        query,
    );

    assert_eq!(strict.status.code(), Some(1), "{strict:?}");
    assert!(strict.stdout.is_empty(), "{strict:?}");
    assert_eq!(String::from_utf8(strict.stderr).unwrap(), expected);
}

#[test]
fn generation_errors_leave_existing_output_unchanged() {
    let root = tempfile::tempdir().unwrap();
    cache(root.path(), "test_lang");
    let query = root.path().join("invalid.ptk");
    fs::write(&query, "Q = (unknown)").unwrap();
    let output = root.path().join("matcher.rs");
    fs::write(&output, "existing matcher").unwrap();

    let invalid = run(
        root.path(),
        &[
            "gen",
            query.to_str().unwrap(),
            "--lang",
            "test_lang",
            "--target",
            "rust",
            "--output",
            output.to_str().unwrap(),
        ],
    );

    assert_eq!(invalid.status.code(), Some(1), "{invalid:?}");
    assert!(invalid.stdout.is_empty(), "{invalid:?}");
    assert_eq!(fs::read_to_string(output).unwrap(), "existing matcher");
    assert_eq!(
        String::from_utf8(invalid.stderr).unwrap(),
        format!(
            "error: `unknown` is not a valid node kind\n --> {}:1:6\n  |\n1 | Q = (unknown)\n  |      ^^^^^^^\n",
            query.display()
        )
    );
}
