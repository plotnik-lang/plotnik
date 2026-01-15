//! Layout snapshot tests.
//!
//! Tests verify cache-line aligned layout and gap-filling optimization.

use crate::Query;
use indoc::indoc;

macro_rules! snap {
    ($query:expr) => {{
        let query = $query.trim();
        let bytecode = Query::expect_valid_bytecode(query);
        insta::with_settings!({
            omit_expression => true
        }, {
            insta::assert_snapshot!(format!("{query}\n---\n{bytecode}"));
        });
    }};
}

#[test]
fn single_instruction() {
    snap!(indoc! {r#"
        Test = (identifier) @id
    "#});
}

#[test]
fn linear_chain() {
    snap!(indoc! {r#"
        Test = (array (identifier) @a (number) @b)
    "#});
}

#[test]
fn branch() {
    snap!(indoc! {r#"
        Test = [(identifier) @id (number) @num]
    "#});
}

#[test]
fn call_return() {
    snap!(indoc! {r#"
        Inner = (identifier) @name
        Test = (array (Inner) @item)
    "#});
}

#[test]
fn cache_line_boundary() {
    // Many small instructions followed by larger one
    snap!(indoc! {r#"
        Test = (array
            (identifier) @a
            (identifier) @b
            (identifier) @c
            (identifier) @d
            (identifier) @e
            [(number) @x (string) @y]
        )
    "#});
}

#[test]
fn large_instruction() {
    // Instruction with many effects/successors
    snap!(indoc! {r#"
        Test = (object
            {(pair) @a (pair) @b (pair) @c (pair) @d}* @items
        )
    "#});
}
