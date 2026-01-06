use super::*;

#[test]
#[cfg(feature = "lang-javascript")]
fn lang_from_name() {
    assert_eq!(from_name("js").unwrap().name(), "javascript");
    assert_eq!(from_name("JavaScript").unwrap().name(), "javascript");
    assert!(from_name("unknown").is_none());
}

#[test]
#[cfg(feature = "lang-go")]
fn lang_from_name_golang() {
    assert_eq!(from_name("go").unwrap().name(), "go");
    assert_eq!(from_name("golang").unwrap().name(), "go");
    assert_eq!(from_name("GOLANG").unwrap().name(), "go");
}

#[test]
#[cfg(feature = "lang-javascript")]
fn lang_from_extension() {
    assert_eq!(from_ext("js").unwrap().name(), "javascript");
    assert_eq!(from_ext("mjs").unwrap().name(), "javascript");
}

#[test]
#[cfg(all(feature = "lang-typescript", feature = "lang-tsx"))]
fn typescript_and_tsx() {
    assert_eq!(typescript().name(), "typescript");
    assert_eq!(tsx().name(), "tsx");
    assert_eq!(from_ext("ts").unwrap().name(), "typescript");
    assert_eq!(from_ext("tsx").unwrap().name(), "tsx");
}

#[test]
fn all_returns_enabled_langs() {
    let langs = all();
    assert!(!langs.is_empty());
    for lang in &langs {
        assert!(!lang.name().is_empty());
    }
}

#[test]
#[cfg(feature = "lang-javascript")]
fn resolve_node_and_field() {
    let lang = javascript();

    let func_id = lang.resolve_named_node("function_declaration");
    assert!(func_id.is_some());

    let unknown = lang.resolve_named_node("nonexistent_node_type");
    assert!(unknown.is_none());

    let name_field = lang.resolve_field("name");
    assert!(name_field.is_some());

    let unknown_field = lang.resolve_field("nonexistent_field");
    assert!(unknown_field.is_none());
}

#[test]
#[cfg(feature = "lang-javascript")]
fn supertype_via_lang_trait() {
    let lang = javascript();

    let expr_id = lang.resolve_named_node("expression").unwrap();
    assert!(lang.is_supertype(expr_id));

    let subtypes = lang.subtypes(expr_id);
    assert!(!subtypes.is_empty());

    let func_id = lang.resolve_named_node("function_declaration").unwrap();
    assert!(!lang.is_supertype(func_id));
}

#[test]
#[cfg(feature = "lang-javascript")]
fn field_validation_via_trait() {
    let lang = javascript();

    let func_id = lang.resolve_named_node("function_declaration").unwrap();
    let name_field = lang.resolve_field("name").unwrap();
    let body_field = lang.resolve_field("body").unwrap();

    assert!(lang.has_field(func_id, name_field));
    assert!(lang.has_field(func_id, body_field));

    let identifier_id = lang.resolve_named_node("identifier").unwrap();
    assert!(lang.is_valid_field_type(func_id, name_field, identifier_id));

    let statement_block_id = lang.resolve_named_node("statement_block").unwrap();
    assert!(lang.is_valid_field_type(func_id, body_field, statement_block_id));
}

#[test]
#[cfg(feature = "lang-javascript")]
fn root_via_trait() {
    let lang = javascript();
    let root_id = lang.root();
    assert!(root_id.is_some());

    let program_id = lang.resolve_named_node("program");
    assert_eq!(root_id, program_id);
}

#[test]
#[cfg(feature = "lang-javascript")]
fn unresolved_returns_none() {
    let lang = javascript();

    assert!(lang.resolve_named_node("nonexistent_node_type").is_none());
    assert!(lang.resolve_field("nonexistent_field").is_none());
}

#[test]
#[cfg(feature = "lang-rust")]
fn rust_lang_works() {
    let lang = rust();
    let func_id = lang.resolve_named_node("function_item");
    assert!(func_id.is_some());
}

#[test]
#[cfg(feature = "lang-javascript")]
fn resolve_nonexistent_nodes() {
    let lang = javascript();

    // Non-existent nodes return None
    assert!(lang.resolve_named_node("end").is_none());
    assert!(lang.resolve_named_node("fake_named").is_none());
    assert!(lang.resolve_anonymous_node("totally_fake_node").is_none());

    // Field resolution
    assert!(lang.resolve_field("name").is_some());
    assert!(lang.resolve_field("fake_field").is_none());
}

/// Verifies that languages with "end" keyword assign it a non-zero ID.
#[test]
#[cfg(all(feature = "lang-ruby", feature = "lang-lua"))]
fn end_keyword_resolves() {
    // Ruby has "end" keyword for blocks, methods, classes, etc.
    let ruby = ruby();
    let ruby_end = ruby.resolve_anonymous_node("end");
    assert!(ruby_end.is_some(), "Ruby should have 'end' keyword");

    // Lua has "end" keyword for blocks, functions, etc.
    let lua = lua();
    let lua_end = lua.resolve_anonymous_node("end");
    assert!(lua_end.is_some(), "Lua should have 'end' keyword");
}
