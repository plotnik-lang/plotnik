use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const HEADER: &str = "// Generated. Do not edit.\n";
const GRAMMAR: &str = r#"{"name":"test_lang","rules":{"program":{"type":"REPEAT","content":{"type":"SYMBOL","name":"identifier"}},"identifier":{"type":"PATTERN","value":"[a-z]+"}}}"#;
const QUERY: &str = "// @test_lang\nQ = (program (identifier) @name :: text)";

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().expect("create fixture root");
    let cache = root.path().join("cache");
    for name in ["test_lang", "other"] {
        let dir = cache.join(name);
        fs::create_dir_all(&dir).expect("create grammar cache");
        fs::write(dir.join("grammar.json"), GRAMMAR.replace("test_lang", name))
            .expect("write grammar fixture");
        fs::write(dir.join("parser.wasm"), b"not wasm").expect("write invalid parser fixture");
    }
    let folder = root.path().join("queries");
    fs::create_dir(&folder).expect("create query folder");
    (root, cache, folder)
}

fn run(folder: &Path, cache: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plotnik"))
        .current_dir(folder)
        .args(args)
        .args(["--color", "never", "--cache-dir"])
        .arg(cache)
        .output()
        .expect("run plotnik")
}

fn success(output: Output) {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
}

#[test]
fn default_folder_generates_owned_modules_and_index_without_recursing() {
    let (_root, cache, folder) = fixture();
    fs::write(folder.join("z.ptk"), QUERY).unwrap();
    fs::write(
        folder.join("a.ptk"),
        "// @test_lang\nA = (identifier) @text :: text",
    )
    .unwrap();
    fs::create_dir(folder.join("nested")).unwrap();
    fs::write(folder.join("nested/broken.ptk"), "invalid").unwrap();
    fs::create_dir(folder.join("directory.ptk")).unwrap();
    fs::create_dir(folder.join("mod")).unwrap();
    fs::write(folder.join("mod/mod.rs"), b"// Handwritten.\n").unwrap();

    success(run(&folder, &cache, &["gen"]));

    for name in ["a", "z"] {
        assert!(
            fs::read_to_string(folder.join(format!("{name}.rs")))
                .unwrap()
                .starts_with(HEADER)
        );
    }
    assert_eq!(
        fs::read_to_string(folder.join("mod.rs")).unwrap(),
        format!("{HEADER}pub mod a;\npub mod z;\n")
    );
    assert!(!folder.join("nested/broken.rs").exists());

    let before = fs::metadata(folder.join("z.rs"))
        .unwrap()
        .modified()
        .unwrap();
    success(run(&folder, &cache, &["gen", "."]));
    assert_eq!(
        fs::metadata(folder.join("z.rs"))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );

    fs::remove_file(folder.join("a.ptk")).unwrap();
    success(run(&folder, &cache, &["gen"]));
    assert!(folder.join("a.rs").exists());
    assert_eq!(
        fs::read_to_string(folder.join("mod.rs")).unwrap(),
        format!("{HEADER}pub mod z;\n")
    );
    assert_eq!(
        fs::read(folder.join("mod/mod.rs")).unwrap(),
        b"// Handwritten.\n"
    );
}

#[test]
fn tags_and_fallback_form_disjoint_groups_with_crossfile_definitions() {
    let (_root, cache, folder) = fixture();
    fs::write(folder.join("name.ptk"), "Name = (identifier) @text :: text").unwrap();
    fs::write(
        folder.join("query.ptk"),
        "// @test_lang\nQ = (program (Name) @name)",
    )
    .unwrap();
    fs::write(
        folder.join("foreign.ptk"),
        "// @other\nName = (program) @node",
    )
    .unwrap();

    success(run(&folder, &cache, &["gen", "--lang", "test_lang"]));

    let query = fs::read_to_string(folder.join("query.rs")).unwrap();
    assert!(query.contains("super::name::"), "{query}");
    let foreign = fs::read_to_string(folder.join("foreign.rs")).unwrap();
    assert!(foreign.contains("other"), "{foreign}");
    assert_eq!(
        fs::read_to_string(folder.join("mod.rs")).unwrap(),
        format!("{HEADER}pub mod foreign;\npub mod name;\npub mod query;\n")
    );
}

#[test]
fn tags_do_not_need_or_use_fallback() {
    let (_root, cache, folder) = fixture();
    fs::write(folder.join("query.ptk"), QUERY).unwrap();

    success(run(&folder, &cache, &["gen", "--lang", "invalid@"]));

    assert!(folder.join("query.rs").exists());
}

#[test]
fn named_tag_and_repository_fallback_share_identical_grammar_and_definitions() {
    for tagged_definition in [false, true] {
        let (_root, cache, folder) = fixture();
        let alias = cache.join("owner%2Frepo@main");
        fs::create_dir(&alias).unwrap();
        fs::write(alias.join("grammar.json"), GRAMMAR).unwrap();
        fs::write(alias.join("parser.wasm"), b"not wasm").unwrap();
        let (name, query) = if tagged_definition {
            (
                "// @test_lang\nName = (identifier) @text :: text",
                "Q = (program (Name) @name)",
            )
        } else {
            (
                "Name = (identifier) @text :: text",
                "// @test_lang\nQ = (program (Name) @name)",
            )
        };
        fs::write(folder.join("name.ptk"), name).unwrap();
        fs::write(folder.join("query.ptk"), query).unwrap();

        success(run(&folder, &cache, &["gen", "--lang", "owner/repo@main"]));

        let query = fs::read_to_string(folder.join("query.rs")).unwrap();
        assert!(query.contains("super::name::"), "{query}");
        assert!(folder.join("name.rs").exists());
        assert_eq!(
            fs::read_to_string(folder.join("mod.rs")).unwrap(),
            format!("{HEADER}pub mod name;\npub mod query;\n")
        );
    }
}

#[test]
fn same_name_grammars_with_different_content_keep_definitions_separate() {
    let (_root, cache, folder) = fixture();
    let alias = cache.join("owner%2Frepo@main");
    fs::create_dir(&alias).unwrap();
    fs::write(
        alias.join("grammar.json"),
        GRAMMAR.replace("identifier", "word"),
    )
    .unwrap();
    fs::write(alias.join("parser.wasm"), b"not wasm").unwrap();
    fs::write(
        folder.join("named.ptk"),
        "// @test_lang\nName = (identifier) @text :: text",
    )
    .unwrap();
    fs::write(folder.join("repository.ptk"), "Name = (word) @text :: text").unwrap();

    success(run(&folder, &cache, &["gen", "--lang", "owner/repo@main"]));

    assert!(folder.join("named.rs").exists());
    assert!(folder.join("repository.rs").exists());
    assert_eq!(
        fs::read_to_string(folder.join("mod.rs")).unwrap(),
        format!("{HEADER}pub mod named;\npub mod repository;\n")
    );
}

#[test]
fn discarded_annotations_do_not_claim_shared_type_ownership() {
    for (live, discarded) in [
        (
            "(identifier) @node :: Shared",
            "(program {(identifier) @node :: Shared} @_)",
        ),
        (
            "(program {(identifier) @node}* @rows :: Shared)",
            "{(program {(identifier) @node}* @rows :: Shared)} @_",
        ),
    ] {
        let (_root, cache, folder) = fixture();
        fs::write(folder.join("a.ptk"), format!("A = {live}")).unwrap();
        fs::write(folder.join("b.ptk"), format!("B = {discarded}")).unwrap();

        success(run(&folder, &cache, &["gen", "--lang", "test_lang"]));

        assert!(folder.join("a.rs").exists());
        assert!(folder.join("b.rs").exists());
        assert_eq!(
            fs::read_to_string(folder.join("mod.rs")).unwrap(),
            format!("{HEADER}pub mod a;\npub mod b;\n")
        );
    }
}

#[cfg(unix)]
#[test]
fn generation_uses_umask_and_preserves_existing_permissions() {
    use std::os::unix::fs::PermissionsExt;

    for (umask, expected) in [("022", 0o644), ("027", 0o640), ("077", 0o600)] {
        let (_root, cache, folder) = fixture();
        fs::write(folder.join("query.ptk"), QUERY).unwrap();
        let generate = || {
            Command::new("sh")
                .current_dir(&folder)
                .args([
                    "-c",
                    "umask \"$1\"; shift; exec \"$@\"",
                    "plotnik-test",
                    umask,
                ])
                .arg(env!("CARGO_BIN_EXE_plotnik"))
                .args(["gen", "--color", "never", "--cache-dir"])
                .arg(&cache)
                .output()
                .unwrap()
        };

        success(generate());

        for name in ["query.rs", "mod.rs"] {
            assert_eq!(
                fs::metadata(folder.join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                expected
            );
        }

        let original = fs::read(folder.join("query.rs")).unwrap();
        fs::set_permissions(folder.join("query.rs"), fs::Permissions::from_mode(0o640)).unwrap();
        fs::set_permissions(folder.join("mod.rs"), fs::Permissions::from_mode(0o664)).unwrap();
        fs::write(folder.join("query.ptk"), QUERY.replace("@name", "@renamed")).unwrap();
        fs::write(
            folder.join("added.ptk"),
            "// @test_lang\nAdded = (identifier)",
        )
        .unwrap();

        success(generate());

        assert_ne!(fs::read(folder.join("query.rs")).unwrap(), original);
        for (name, mode) in [
            ("query.rs", 0o640),
            ("mod.rs", 0o664),
            ("added.rs", expected),
        ] {
            assert_eq!(
                fs::metadata(folder.join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                mode
            );
        }
        assert_eq!(
            fs::read_to_string(folder.join("mod.rs")).unwrap(),
            format!("{HEADER}pub mod added;\npub mod query;\n")
        );
    }
}

#[test]
fn ambiguous_shared_types_report_all_declarations_without_publication() {
    for body in [
        "(identifier) @value :: Shared",
        "(program {(identifier) @value}* @entries :: Shared)",
    ] {
        let (_root, cache, folder) = fixture();
        fs::write(folder.join("a.ptk"), format!("A = {body}")).unwrap();
        fs::write(folder.join("b.ptk"), format!("B = {body}")).unwrap();

        let output = run(&folder, &cache, &["gen", "--lang", "test_lang"]);

        assert_eq!(output.status.code(), Some(1), "{body}: {output:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        for expected in [
            "ambiguous source ownership",
            "Shared",
            "a.ptk",
            "b.ptk",
            "Define the type in one query file",
        ] {
            assert!(stderr.contains(expected), "{stderr}");
        }
        assert!(!folder.join("a.rs").exists());
        assert!(!folder.join("b.rs").exists());
        assert!(!folder.join("mod.rs").exists());
    }
}

#[test]
fn inferred_name_collisions_require_an_owner_in_either_file_order() {
    for (a, b) in [("a.ptk", "b.ptk"), ("z.ptk", "a.ptk")] {
        let (_root, cache, folder) = fixture();
        fs::write(folder.join(a), "A = (program {(identifier) @value}* @b_c)").unwrap();
        fs::write(folder.join(b), "AB = (program {(identifier) @value}* @c)").unwrap();

        let output = run(&folder, &cache, &["gen", "--lang", "test_lang"]);

        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        for expected in [
            "ambiguous source ownership",
            "ABC",
            a,
            b,
            "Define the type in one query file",
        ] {
            assert!(stderr.contains(expected), "{stderr}");
        }
        assert!(!folder.join(a).with_extension("rs").exists());
        assert!(!folder.join(b).with_extension("rs").exists());
        assert!(!folder.join("mod.rs").exists());

        fs::write(
            folder.join(a),
            "A = (program {(identifier) @value}* @b_c :: ABC)",
        )
        .unwrap();

        success(run(
            &folder,
            &cache,
            &["check", ".", "--lang", "test_lang", "--strict"],
        ));
        success(run(&folder, &cache, &["gen", "--lang", "test_lang"]));
        assert!(folder.join(a).with_extension("rs").exists());
        assert!(folder.join(b).with_extension("rs").exists());
    }
}

#[test]
fn check_and_gen_reject_inferred_and_explicit_type_cycles_without_writes() {
    for annotation in ["", " :: BEntries"] {
        let (_root, cache, folder) = fixture();
        fs::write(
            folder.join("a.ptk"),
            "Q = (B)\nBEntries = (identifier) @value :: text",
        )
        .unwrap();
        fs::write(
            folder.join("b.ptk"),
            format!("B = (program {{(identifier) @value :: text}}* @entries{annotation})"),
        )
        .unwrap();
        for name in ["a.rs", "b.rs", "mod.rs"] {
            fs::write(folder.join(name), format!("{HEADER}existing")).unwrap();
        }

        for command in ["check", "gen"] {
            let output = run(&folder, &cache, &[command, ".", "--lang", "test_lang"]);

            assert_eq!(output.status.code(), Some(1), "{output:?}");
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(
                stderr.contains("source files form a dependency cycle"),
                "{stderr}"
            );
            for name in ["a.rs", "b.rs", "mod.rs"] {
                assert_eq!(
                    fs::read_to_string(folder.join(name)).unwrap(),
                    format!("{HEADER}existing")
                );
            }
        }
    }
}

#[test]
fn warnings_allow_owned_outputs_to_be_replaced() {
    let (_root, cache, folder) = fixture();
    fs::write(
        folder.join("query.ptk"),
        "// @test_lang\nQ = (program [A: (identifier) B: (identifier)])",
    )
    .unwrap();
    fs::write(folder.join("query.rs"), format!("{HEADER}old source")).unwrap();
    fs::write(folder.join("mod.rs"), format!("{HEADER}old index")).unwrap();

    let output = run(&folder, &cache, &["gen"]);

    assert!(String::from_utf8_lossy(&output.stderr).contains("warning: alternative labels"));
    success(output);
    let source = fs::read_to_string(folder.join("query.rs")).unwrap();
    assert!(source.starts_with(HEADER));
    assert!(!source.contains("old source"));
    assert_eq!(
        fs::read_to_string(folder.join("mod.rs")).unwrap(),
        format!("{HEADER}pub mod query;\n")
    );
}

#[test]
fn missing_and_malformed_tags_fail_before_any_publication() {
    for text in ["Q = (program)", "// @bad/name\nQ = (program)"] {
        let (_root, cache, folder) = fixture();
        fs::write(folder.join("a.ptk"), QUERY).unwrap();
        fs::write(folder.join("z.ptk"), text).unwrap();

        let output = run(&folder, &cache, &["gen"]);

        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(!folder.join("a.rs").exists());
        assert!(!folder.join("mod.rs").exists());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("z.ptk"), "{stderr}");
    }
}

#[test]
fn later_group_failure_does_not_publish_earlier_groups() {
    let (_root, cache, folder) = fixture();
    fs::write(folder.join("a.ptk"), "// @other\nA = (program) @node").unwrap();
    fs::write(folder.join("z.ptk"), "// @test_lang\nQ = (unknown)").unwrap();

    let output = run(&folder, &cache, &["gen"]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(!folder.join("a.rs").exists());
    assert!(!folder.join("mod.rs").exists());
}

#[test]
fn handwritten_destination_and_nested_module_collision_leave_all_outputs_unchanged() {
    for nested in [false, true] {
        let (_root, cache, folder) = fixture();
        fs::write(folder.join("a.ptk"), QUERY).unwrap();
        fs::write(folder.join("z.ptk"), "// @test_lang\nZ = (program) @node").unwrap();
        fs::write(folder.join("a.rs"), format!("{HEADER}old")).unwrap();
        fs::write(folder.join("mod.rs"), format!("{HEADER}old index")).unwrap();
        if nested {
            fs::create_dir(folder.join("z")).unwrap();
            fs::write(folder.join("z/mod.rs"), "// manual").unwrap();
        } else {
            fs::write(folder.join("z.rs"), "// manual").unwrap();
        }

        let output = run(&folder, &cache, &["gen"]);

        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert_eq!(
            fs::read_to_string(folder.join("a.rs")).unwrap(),
            format!("{HEADER}old")
        );
        assert_eq!(
            fs::read_to_string(folder.join("mod.rs")).unwrap(),
            format!("{HEADER}old index")
        );
        let target = if nested { "z/mod.rs" } else { "z.rs" };
        assert_eq!(
            fs::read_to_string(folder.join(target)).unwrap(),
            "// manual"
        );
    }
}

#[test]
fn manual_folder_and_sibling_indexes_are_not_modified() {
    for sibling in [false, true] {
        let (_root, cache, folder) = fixture();
        fs::write(folder.join("query.ptk"), QUERY).unwrap();
        let index = if sibling {
            folder.with_extension("rs")
        } else {
            folder.join("mod.rs")
        };
        fs::write(&index, "// manual module index").unwrap();

        let output = run(&folder, &cache, &["gen"]);

        assert!(String::from_utf8_lossy(&output.stderr).contains("pub mod query;"));
        success(output);
        assert_eq!(
            fs::read_to_string(&index).unwrap(),
            "// manual module index"
        );
        assert!(folder.join("query.rs").exists());
        if sibling {
            assert!(!folder.join("mod.rs").exists());
        }
    }
}

#[cfg(unix)]
#[test]
fn sibling_index_does_not_require_read_permission() {
    use std::os::unix::fs::PermissionsExt;

    let (_root, cache, folder) = fixture();
    fs::write(folder.join("query.ptk"), QUERY).unwrap();
    let sibling = folder.with_extension("rs");
    let content = b"// Handwritten module index.\n";
    fs::write(&sibling, content).unwrap();
    fs::set_permissions(&sibling, fs::Permissions::from_mode(0o000)).unwrap();

    let output = run(&folder, &cache, &["gen"]);

    let mode = fs::metadata(&sibling).unwrap().permissions().mode() & 0o777;
    fs::set_permissions(&sibling, fs::Permissions::from_mode(0o600)).unwrap();
    success(output);
    assert_eq!(mode, 0o000);
    assert_eq!(fs::read(&sibling).unwrap(), content);
    assert!(folder.join("query.rs").exists());
    assert!(!folder.join("mod.rs").exists());
}

#[test]
fn sibling_index_directory_is_rejected_before_writes() {
    let (_root, cache, folder) = fixture();
    fs::write(folder.join("query.ptk"), QUERY).unwrap();
    let existing = format!("{HEADER}old source");
    fs::write(folder.join("query.rs"), &existing).unwrap();
    let sibling = folder.with_extension("rs");
    fs::create_dir(&sibling).unwrap();

    let output = run(&folder, &cache, &["gen"]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("refusing non-file generation destination"),
        "{stderr}"
    );
    assert!(stderr.contains(sibling.to_str().unwrap()), "{stderr}");
    assert_eq!(
        fs::read_to_string(folder.join("query.rs")).unwrap(),
        existing
    );
    assert!(!folder.join("mod.rs").exists());
    assert!(sibling.is_dir());
    assert_eq!(fs::read_dir(&sibling).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 2);
}

#[test]
fn keywords_use_raw_module_identifiers() {
    let (_root, cache, folder) = fixture();
    fs::write(folder.join("type.ptk"), QUERY).unwrap();

    success(run(&folder, &cache, &["gen"]));

    assert!(folder.join("type.rs").exists());
    assert_eq!(
        fs::read_to_string(folder.join("mod.rs")).unwrap(),
        format!("{HEADER}pub mod r#type;\n")
    );
}

#[test]
fn competing_folder_and_sibling_indexes_are_rejected_before_writes() {
    let (_root, cache, folder) = fixture();
    fs::write(folder.join("query.ptk"), QUERY).unwrap();
    fs::write(folder.with_extension("rs"), "// sibling index").unwrap();
    fs::write(folder.join("mod.rs"), "// folder index").unwrap();

    let output = run(&folder, &cache, &["gen"]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(!folder.join("query.rs").exists());
    assert_eq!(
        fs::read_to_string(folder.with_extension("rs")).unwrap(),
        "// sibling index"
    );
    assert_eq!(
        fs::read_to_string(folder.join("mod.rs")).unwrap(),
        "// folder index"
    );
}

#[test]
fn invalid_module_stems_and_reserved_index_do_not_write() {
    for name in ["mod", "bad-name", "123", "self"] {
        let (_root, cache, folder) = fixture();
        fs::write(folder.join(format!("{name}.ptk")), QUERY).unwrap();

        let output = run(&folder, &cache, &["gen"]);

        assert!(!output.status.success(), "{output:?}");
        assert!(!folder.join(format!("{name}.rs")).exists());
        assert!(!folder.join("mod.rs").exists());
    }
}

#[cfg(unix)]
#[test]
fn workspace_input_errors_are_reported_without_publication() {
    let (_root, cache, folder) = fixture();
    fs::write(folder.join("query.ptk"), QUERY).unwrap();
    std::os::unix::fs::symlink("broken.ptk", folder.join("broken.ptk")).unwrap();
    let source = format!("{HEADER}old source");
    let index = format!("{HEADER}old index");
    fs::write(folder.join("query.rs"), &source).unwrap();
    fs::write(folder.join("mod.rs"), &index).unwrap();

    for command in ["gen", "check", "dump"] {
        let output = run(&folder, &cache, &[command, ".", "--lang", "test_lang"]);

        assert_eq!(output.status.code(), Some(2), "{command}: {output:?}");
        assert!(output.stdout.is_empty(), "{command}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("broken.ptk"),
            "{command}: {output:?}"
        );
        assert_eq!(fs::read_to_string(folder.join("query.rs")).unwrap(), source);
        assert_eq!(fs::read_to_string(folder.join("mod.rs")).unwrap(), index);
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 4);
    }
}

#[cfg(unix)]
#[test]
fn generation_requires_a_directory_not_a_symlink() {
    let (root, cache, folder) = fixture();
    fs::write(folder.join("query.ptk"), QUERY).unwrap();
    let link = root.path().join("linked");
    std::os::unix::fs::symlink(&folder, &link).unwrap();
    fs::write(root.path().join("linked.rs"), "// manual index").unwrap();

    for suffix in ["", "/", "/."] {
        let path = format!("{}{suffix}", link.display());

        let output = run(&folder, &cache, &["gen", &path]);

        assert_eq!(output.status.code(), Some(2), "{path}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("expected a directory, not a file or symlink"),
            "{output:?}"
        );
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
        assert_eq!(
            fs::read_to_string(root.path().join("linked.rs")).unwrap(),
            "// manual index"
        );
    }
}
