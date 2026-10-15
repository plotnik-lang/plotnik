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

#[test]
fn individual_selection_uses_loaded_name_and_metadata_only() {
    let root = tempfile::tempdir().unwrap();
    cache(root.path(), "test_lang");
    cache(root.path(), "owner%2Frepo@main");

    for command in ["check", "infer", "dump", "gen"] {
        let mut args = vec![command, "-q", QUERY];
        if command == "gen" {
            args.extend(["--target", "rust"]);
        }
        let declared = run(root.path(), &args);
        args.extend(["--lang", "owner/repo@main"]);
        let explicit = run(root.path(), &args);

        assert_eq!(declared.status.code(), Some(0), "{declared:?}");
        assert_eq!(explicit.status.code(), Some(0), "{explicit:?}");
        let declared = String::from_utf8(declared.stdout).unwrap();
        let explicit = String::from_utf8(explicit.stdout).unwrap();
        let explicit = explicit.replace(
            root.path()
                .join("owner%2Frepo@main/grammar.json")
                .to_str()
                .unwrap(),
            root.path().join("test_lang/grammar.json").to_str().unwrap(),
        );
        assert_eq!(declared, explicit);
        if command == "infer" {
            assert!(declared.contains("name: string"), "{declared}");
        }
    }

    let file = root.path().join("query.ptk");
    fs::write(&file, "// @Test_lang\nQ = (program)").unwrap();
    for command in ["check", "run", "trace"] {
        let mut args = vec![command, file.to_str().unwrap(), "--lang", "owner/repo@main"];
        if command != "check" {
            args.extend(["-s", "hello"]);
        }
        let conflict = run(root.path(), &args);

        assert_eq!(conflict.status.code(), Some(1), "{conflict:?}");
        let stderr = String::from_utf8(conflict.stderr).unwrap();
        assert!(stderr.contains("query.ptk:1:5"), "{stderr}");
        assert!(stderr.contains("declared language 'Test_lang' does not match loaded grammar 'test_lang' selected by --lang 'owner/repo@main'"), "{stderr}");
    }

    let unknown_node = run(
        root.path(),
        &["check", "-q", "// @test_lang\nQ = (unknown)"],
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
    fs::write(
        queries.join("a.ptk"),
        "// @other\nName = (identifier) @text :: text",
    )
    .unwrap();
    fs::write(queries.join("z.ptk"), "Q = (program (Name) @name)").unwrap();
    let path = queries.to_str().unwrap();

    let missing = run(root.path(), &["check", path]);
    let merged = run(root.path(), &["check", path, "--lang", "test_lang"]);

    assert_eq!(missing.status.code(), Some(2), "{missing:?}");
    assert_eq!(merged.status.code(), Some(0), "{merged:?}");

    fs::write(queries.join("z.ptk"), "// @bad/name\nQ = (program)").unwrap();
    let missing = run(root.path(), &["check", path]);
    let malformed = run(root.path(), &["check", path, "--lang", "test_lang"]);

    assert_eq!(missing.status.code(), Some(2), "{missing:?}");
    assert!(
        String::from_utf8(missing.stderr)
            .unwrap()
            .contains("directories require --lang")
    );
    assert_eq!(malformed.status.code(), Some(1), "{malformed:?}");
    assert!(
        String::from_utf8(malformed.stderr)
            .unwrap()
            .contains("z.ptk:1:5")
    );
}

#[test]
fn declaration_errors_are_located_and_missing_selection_is_usage_error() {
    let root = tempfile::tempdir().unwrap();

    let malformed = run(root.path(), &["check", "-q", "// @bad/name\nQ = (program)"]);
    let missing = run(root.path(), &["check", "-q", "Q = (program)"]);
    assert_eq!(malformed.status.code(), Some(1), "{malformed:?}");
    let stderr = String::from_utf8(malformed.stderr).unwrap();
    assert!(
        stderr.contains("1 | // @bad/name\n  |     ^^^^^^^^"),
        "{stderr}"
    );
    assert!(stderr.contains("invalid grammar declaration"), "{stderr}");
    assert_eq!(missing.status.code(), Some(2), "{missing:?}");
    assert!(
        String::from_utf8(missing.stderr)
            .unwrap()
            .contains("use --lang or a first-line @language declaration")
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn tree_displays_plotnik_syntax_locally_for_one_input() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("query.ptk");
    fs::write(&file, "// @UnknownLanguage\nQ = (program)").unwrap();
    let path = file.to_str().unwrap();

    let plain = run(root.path(), &["tree", path]);
    let flagged = run(root.path(), &["tree", path, "--lang", "invalid//selector"]);
    let inline = run(root.path(), &["tree", "-q", "Q = (program)"]);
    let cst = run(root.path(), &["tree", path, "--query-view", "cst"]);

    assert_eq!(plain.status.code(), Some(0), "{plain:?}");
    assert_eq!(flagged.status.code(), Some(0), "{flagged:?}");
    assert_eq!(plain.stdout, flagged.stdout);
    assert_eq!(inline.status.code(), Some(0), "{inline:?}");
    assert_eq!(inline.stdout, b"Root\n  Def Q\n    NamedNode program\n");
    assert_eq!(cst.status.code(), Some(0), "{cst:?}");
    assert!(
        String::from_utf8(cst.stdout)
            .unwrap()
            .contains("// @UnknownLanguage")
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_plotnik"))
        .args([
            "tree",
            "-",
            "--lang",
            "plotnik",
            "--color",
            "never",
            "--cache-dir",
        ])
        .arg(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"// @UnknownLanguage\nQ = (program)")
        .unwrap();
    let stdin = child.wait_with_output().unwrap();
    let missing = run(root.path(), &["tree", "-"]);

    assert_eq!(stdin.status.code(), Some(0), "{stdin:?}");
    assert_eq!(
        stdin.stdout,
        b"# <stdin>\nRoot\n  Def Q\n    NamedNode program\n"
    );
    assert_eq!(missing.status.code(), Some(2), "{missing:?}");
    assert!(
        String::from_utf8(missing.stderr)
            .unwrap()
            .contains("input language required: use --lang")
    );

    for args in [
        vec!["tree", path, "extra.js"],
        vec!["tree", path, "-q", "Q = (program)"],
        vec!["tree", path, "-s", "hello"],
        vec!["tree", "-q", "Q = (program)", "-s", "hello"],
    ] {
        let multiple = run(root.path(), &args);

        assert_eq!(multiple.status.code(), Some(2), "{multiple:?}");
        assert!(multiple.stdout.is_empty());
        let stderr = String::from_utf8(multiple.stderr).unwrap();
        assert!(
            stderr.contains("unexpected argument") || stderr.contains("cannot be used with"),
            "{stderr}"
        );
    }

    fs::write(&file, "// @bad/name\nQ = (program)").unwrap();
    let malformed = run(root.path(), &["tree", path]);

    assert_eq!(malformed.status.code(), Some(1), "{malformed:?}");
    assert!(
        String::from_utf8(malformed.stderr)
            .unwrap()
            .contains("query.ptk:1:5")
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}
