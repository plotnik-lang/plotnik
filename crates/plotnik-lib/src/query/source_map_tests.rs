use super::source_map::{SourceId, SourceKind, SourceMap};

#[test]
fn single_one_liner() {
    let map = SourceMap::one_liner("hello world");
    let id = SourceId(0);

    assert_eq!(map.content(id), "hello world");
    assert_eq!(map.kind(id), &SourceKind::OneLiner);
    assert_eq!(map.len(), 1);
}

#[test]
fn stdin_source() {
    let mut map = SourceMap::new();
    let id = map.add_stdin("from stdin");

    assert_eq!(map.content(id), "from stdin");
    assert_eq!(map.kind(id), &SourceKind::Stdin);
}

#[test]
fn file_source() {
    let mut map = SourceMap::new();
    let id = map.add_file("main.ptk", "Foo = (bar)");

    assert_eq!(map.content(id), "Foo = (bar)");
    assert_eq!(map.kind(id), &SourceKind::File("main.ptk".to_owned()));
}

#[test]
fn multiple_sources() {
    let mut map = SourceMap::new();
    let a = map.add_file("a.ptk", "content a");
    let b = map.add_file("b.ptk", "content b");
    let c = map.add_one_liner("inline");
    let d = map.add_stdin("piped");

    assert_eq!(map.len(), 4);
    assert_eq!(map.content(a), "content a");
    assert_eq!(map.content(b), "content b");
    assert_eq!(map.content(c), "inline");
    assert_eq!(map.content(d), "piped");

    assert_eq!(map.kind(a), &SourceKind::File("a.ptk".to_owned()));
    assert_eq!(map.kind(b), &SourceKind::File("b.ptk".to_owned()));
    assert_eq!(map.kind(c), &SourceKind::OneLiner);
    assert_eq!(map.kind(d), &SourceKind::Stdin);
}

#[test]
fn iteration() {
    let mut map = SourceMap::new();
    map.add_file("a.ptk", "aaa");
    map.add_one_liner("bbb");

    let items: Vec<_> = map.iter().collect();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, SourceId(0));
    assert_eq!(items[0].kind, &SourceKind::File("a.ptk".to_owned()));
    assert_eq!(items[0].content, "aaa");
    assert_eq!(items[1].id, SourceId(1));
    assert_eq!(items[1].kind, &SourceKind::OneLiner);
    assert_eq!(items[1].content, "bbb");
}

#[test]
fn get_source() {
    let mut map = SourceMap::new();
    let id = map.add_file("test.ptk", "hello");

    let source = map.get(id);
    assert_eq!(source.id, id);
    assert_eq!(source.kind, &SourceKind::File("test.ptk".to_owned()));
    assert_eq!(source.content, "hello");
    assert_eq!(source.as_str(), "hello");
}

#[test]
fn display_name() {
    assert_eq!(SourceKind::OneLiner.display_name(), "<query>");
    assert_eq!(SourceKind::Stdin.display_name(), "<stdin>");
    assert_eq!(
        SourceKind::File("foo.ptk".to_owned()).display_name(),
        "foo.ptk"
    );
}

#[test]
#[should_panic(expected = "invalid SourceId")]
fn invalid_id_panics() {
    let map = SourceMap::new();
    let _ = map.content(SourceId(999));
}

#[test]
fn multiple_stdin_sources() {
    let mut map = SourceMap::new();
    let a = map.add_stdin("first stdin");
    let b = map.add_stdin("second stdin");

    assert_eq!(map.content(a), "first stdin");
    assert_eq!(map.content(b), "second stdin");
    assert_eq!(map.kind(a), &SourceKind::Stdin);
    assert_eq!(map.kind(b), &SourceKind::Stdin);
}
