#![cfg(feature = "lang-json")]

use plotnik_lib::grammar::DumpOptions;

use crate::language_registry;

/// The canonical end-to-end document: every register (pattern/type/text), the
/// extras line, hidden splices, the category, and the `; root`/`; extra` notes.
#[test]
fn dump_json_document() {
    let dump = language_registry::json()
        .grammar()
        .tree()
        .dump(&DumpOptions::default());
    insta::assert_snapshot!("dump_json", dump);
}
