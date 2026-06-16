//! Bytecode emission tests, organized by language feature.

use crate::shot_bytecode;

#[test]
fn nodes_named() {
    shot_bytecode!(
        r#"
        Test = (identifier) @id
    "#
    );
}

#[test]
fn nodes_anonymous() {
    shot_bytecode!(
        r#"
        Test = (binary_expression "+" @op)
    "#
    );
}

#[test]
fn nodes_wildcard_any() {
    shot_bytecode!(
        r#"
        Test = (pair key: _ @key)
    "#
    );
}

#[test]
fn nodes_wildcard_named() {
    shot_bytecode!(
        r#"
        Test = (pair key: (_) @key)
    "#
    );
}

#[test]
fn nodes_error() {
    shot_bytecode!(
        r#"
        Test = (ERROR) @err
    "#
    );
}

#[test]
fn nodes_missing() {
    shot_bytecode!(
        r#"
        Test = (MISSING) @m
    "#
    );
}

#[test]
fn captures_basic() {
    shot_bytecode!(
        r#"
        Test = (identifier) @name
    "#
    );
}

#[test]
fn captures_multiple() {
    shot_bytecode!(
        r#"
        Test = (binary_expression (identifier) @a (number) @b)
    "#
    );
}

#[test]
fn captures_nested_flat() {
    shot_bytecode!(
        r#"
        Test = (array (array (identifier) @c) @b) @a
    "#
    );
}

#[test]
fn captures_deeply_nested() {
    shot_bytecode!(
        r#"
        Test = (array (array (array (identifier) @d) @c) @b) @a
    "#
    );
}

#[test]
fn captures_with_type_string() {
    shot_bytecode!(
        r#"
        Test = (identifier) @name :: string
    "#
    );
}

#[test]
fn captures_with_type_custom() {
    shot_bytecode!(
        r#"
        Test = (identifier) @name :: Identifier
    "#
    );
}

#[test]
fn captures_struct_scope() {
    shot_bytecode!(
        r#"
        Test = {(identifier) @a (number) @b} @item
    "#
    );
}

#[test]
fn captures_wrapper_struct() {
    shot_bytecode!(
        r#"
        Test = {{(identifier) @id (number) @num} @row}* @rows
    "#
    );
}

#[test]
fn captures_optional_wrapper_struct() {
    shot_bytecode!(
        r#"
        Test = {{(identifier) @id} @inner}? @outer
    "#
    );
}

#[test]
fn captures_struct_with_type_annotation() {
    shot_bytecode!(
        r#"
        Test = {(identifier) @fn} @outer :: FunctionInfo
    "#
    );
}

#[test]
fn captures_enum_with_type_annotation() {
    shot_bytecode!(
        r#"
        Test = [A: (identifier) @id B: (number) @num] @expr :: Expression
    "#
    );
}

#[test]
fn fields_single() {
    shot_bytecode!(
        r#"
        Test = (function_declaration name: (identifier) @name)
    "#
    );
}

#[test]
fn fields_multiple() {
    shot_bytecode!(
        r#"
        Test = (binary_expression
            left: (_) @left
            right: (_) @right)
    "#
    );
}

#[test]
fn fields_negated() {
    shot_bytecode!(
        r#"
        Test = (import_specifier name: (identifier) @key -alias)
    "#
    );
}

#[test]
fn fields_alternation() {
    shot_bytecode!(
        r#"
        Test = (call_expression function: [(identifier) @fn (number) @num])
    "#
    );
}

#[test]
fn quantifiers_optional() {
    shot_bytecode!(
        r#"
        Test = (function_declaration (decorator)? @dec)
    "#
    );
}

#[test]
fn quantifiers_star() {
    shot_bytecode!(
        r#"
        Test = (identifier)* @items
    "#
    );
}

#[test]
fn quantifiers_plus() {
    shot_bytecode!(
        r#"
        Test = (identifier)+ @items
    "#
    );
}

#[test]
fn quantifiers_optional_nongreedy() {
    shot_bytecode!(
        r#"
        Test = (function_declaration (decorator)?? @dec)
    "#
    );
}

#[test]
fn quantifiers_star_nongreedy() {
    shot_bytecode!(
        r#"
        Test = (identifier)*? @items
    "#
    );
}

#[test]
fn quantifiers_plus_nongreedy() {
    shot_bytecode!(
        r#"
        Test = (identifier)+? @items
    "#
    );
}

#[test]
fn quantifiers_struct_array() {
    shot_bytecode!(
        r#"
        Test = (array {(identifier) @a (number) @b}* @items)
    "#
    );
}

#[test]
fn quantifiers_first_child_array() {
    shot_bytecode!(
        r#"
        Test = (array (identifier)* @ids (number) @n)
    "#
    );
}

#[test]
fn quantifiers_repeat_navigation() {
    shot_bytecode!(
        r#"
        Test = (function_declaration (decorator)* @decs)
    "#
    );
}

#[test]
fn quantifiers_sequence_in_called_def() {
    shot_bytecode!(
        r#"
        Item = (identifier) @name
        Collect = {(Item) @item}* @items
        Test = (array (Collect))
    "#
    );
}

#[test]
fn sequences_basic() {
    shot_bytecode!(
        r#"
        Test = (array {(identifier) (number)})
    "#
    );
}

#[test]
fn sequences_with_captures() {
    shot_bytecode!(
        r#"
        Test = (array {(identifier) @a (number) @b})
    "#
    );
}

#[test]
fn sequences_nested() {
    shot_bytecode!(
        r#"
        Test = (array {(identifier) {(number) (string)} (null)})
    "#
    );
}

#[test]
fn sequences_in_quantifier() {
    shot_bytecode!(
        r#"
        Test = (array {(identifier) @id (number) @num}* @items)
    "#
    );
}

#[test]
fn alternations_unlabeled() {
    shot_bytecode!(
        r#"
        Test = [(identifier) @id (string) @str]
    "#
    );
}

#[test]
fn alternations_labeled() {
    shot_bytecode!(
        r#"
        Test = [
            A: (identifier) @a
            B: (number) @b
        ]
    "#
    );
}

#[test]
fn alternations_null_injection() {
    shot_bytecode!(
        r#"
        Test = [(identifier) @x (number) @y]
    "#
    );
}

#[test]
fn alternations_captured() {
    shot_bytecode!(
        r#"
        Test = [(identifier) (number)] @value
    "#
    );
}

#[test]
fn alternations_captured_tagged() {
    shot_bytecode!(
        r#"
        Test = [A: (identifier) @a  B: (number) @b] @item
    "#
    );
}

#[test]
fn alternations_tagged_with_definition_ref() {
    shot_bytecode!(
        r#"
        Inner = (identifier) @name
        Test = [A: (Inner)  B: (number) @b] @item
    "#
    );
}

#[test]
fn alternations_in_quantifier() {
    shot_bytecode!(
        r#"
        Test = (object { [A: (pair) @a  B: (shorthand_property_identifier) @b] @item }* @items)
    "#
    );
}

#[test]
fn alternations_no_internal_captures() {
    shot_bytecode!(
        r#"
        Test = (program [(identifier) (number)] @x)
    "#
    );
}

#[test]
fn alternations_in_child_position() {
    shot_bytecode!(
        r#"
        Test = (program [(expression_statement) @expr])
    "#
    );
}

#[test]
fn alternations_tagged_in_field_constraint() {
    shot_bytecode!(
        r#"
        Test = (pair key: [A: (identifier) @a B: (number)] @kind)
    "#
    );
}

#[test]
fn anchors_between_siblings() {
    shot_bytecode!(
        r#"
        Test = (array (identifier) . (number))
    "#
    );
}

#[test]
fn anchors_between_siblings_with_alternation() {
    shot_bytecode!(
        r#"
        Test = (program (expression_statement (binary_expression (identifier) @left . [(identifier) @right])))
    "#
    );
}

#[test]
fn anchors_first_child() {
    shot_bytecode!(
        r#"
        Test = (array . (identifier))
    "#
    );
}

#[test]
fn anchors_first_child_with_anonymous() {
    shot_bytecode!(
        r#"
        Test = (array . "+")
    "#
    );
}

#[test]
fn anchors_first_child_with_captured_anonymous() {
    shot_bytecode!(
        r#"
        Test = (array . "+" @op)
    "#
    );
}

#[test]
fn anchors_strict_first_child() {
    shot_bytecode!(
        r#"
        Test = (array .! (identifier))
    "#
    );
}

#[test]
fn anchors_first_child_with_alternation() {
    shot_bytecode!(
        r#"
        Test = (program (expression_statement (call_expression arguments: (arguments . [(identifier) @arg]))))
    "#
    );
}

#[test]
fn anchors_last_child() {
    shot_bytecode!(
        r#"
        Test = (array (identifier) .)
    "#
    );
}

#[test]
fn anchors_last_child_with_anonymous() {
    shot_bytecode!(
        r#"
        Test = (array "+" .)
    "#
    );
}

#[test]
fn anchors_last_child_with_captured_anonymous() {
    shot_bytecode!(
        r#"
        Test = (array "+" @op .)
    "#
    );
}

#[test]
fn anchors_strict_last_child() {
    shot_bytecode!(
        r#"
        Test = (array (identifier) .!)
    "#
    );
}

#[test]
fn anchors_with_anonymous() {
    shot_bytecode!(
        r#"
        Test = (binary_expression "+" . (identifier))
    "#
    );
}

#[test]
fn anchors_with_captured_anonymous() {
    shot_bytecode!(
        r#"
        Test = (binary_expression "+" @op . (identifier))
    "#
    );
}

#[test]
fn anchors_with_alternation_anonymous() {
    shot_bytecode!(
        r#"
        Test = (binary_expression (identifier) . ["+"])
    "#
    );
}

#[test]
fn anchors_with_mixed_alternation_classifies_each_branch() {
    shot_bytecode!(
        r#"
        Test = {(identifier) . [(number) "+"]}
    "#
    );
}

#[test]
fn anchors_after_mixed_alternation_classifies_each_branch() {
    // A soft anchor after a mixed alternation must classify per branch: the named
    // `(number)` branch reaches the `(identifier)` follower via `NextSkip` (─•─),
    // the anonymous `"+"` branch via `NextSkipExtras` (─◦─). The follower's entry
    // is cloned so both navs converge on the same successor.
    shot_bytecode!(
        r#"
        Test = {[(number) "+"] . (identifier)}
    "#
    );
}

#[test]
fn anchors_after_alternation_anonymous_follower_stays_conservative() {
    // The follower `","` is anonymous, so both-sides-named never holds and every
    // branch keeps extras-only adjacency — no follower twin is emitted.
    shot_bytecode!(
        r#"
        Test = {[(number) (string)] . ","}
    "#
    );
}

#[test]
fn anchors_after_alternation_captured_named_follower() {
    // The follower carries a capture; cloning its entry duplicates the capture
    // effects, but only the matched branch's path runs them.
    shot_bytecode!(
        r#"
        Test = {[(number) "+"] . (identifier) @id}
    "#
    );
}

#[test]
fn anchors_after_inline_captured_alternation_splits() {
    // A scalar capture's effects ride the branch instructions, leaving the exit as
    // the follower's `Match` — so the split still fires: `(number)` reaches the
    // follower via `NextSkip`, `"+"` via `NextSkipExtras`.
    shot_bytecode!(
        r#"
        Test = {[(number) "+"] @t . (identifier)}
    "#
    );
}

#[test]
fn anchors_after_captured_alternation_stays_conservative() {
    // When the alternation itself is captured (here, tagged with `@t`), its exit is
    // an effect-bearing epsilon — the `Set` for the bound value — rather than the
    // follower's `Match`, so no twin is emitted and every branch keeps extras-only
    // adjacency. Documented follower-side limitation pending follow-up.
    shot_bytecode!(
        r#"
        Test = {[A: (number) B: "+"] @t . (identifier)}
    "#
    );
}

#[test]
fn anchors_after_alternation_quantified_branch_stays_conservative() {
    // A quantified branch's zero-match path leaves no named node on the anchor's
    // left, so the soft-skip upgrade is unsound; the `(identifier)?` branch keeps
    // extras-only adjacency rather than routing to a `NextSkip` twin. Quantified
    // followers are a separate deferred case.
    shot_bytecode!(
        r#"
        Test = (array [(identifier)? @x ";"] . (identifier) @y)
    "#
    );
}

#[test]
fn anchors_with_ref_to_anonymous() {
    shot_bytecode!(
        r#"
        Comma = ","
        Test = (array (Comma) . (string) @next)
    "#
    );
}

#[test]
fn anchors_with_quantified_anonymous() {
    shot_bytecode!(
        r#"
        Test = (array (identifier) . ","?)
    "#
    );
}

#[test]
fn anchors_with_field_anonymous() {
    shot_bytecode!(
        r#"
        Test = (pair key: (property_identifier) . value: _)
    "#
    );
}

#[test]
fn anchors_with_sequence_anonymous() {
    shot_bytecode!(
        r#"
        Test = (array (identifier) . {"," (string)})
    "#
    );
}

#[test]
fn anchors_strict_between_siblings() {
    shot_bytecode!(
        r#"
        Test = (binary_expression "+" .! (identifier))
    "#
    );
}

#[test]
fn anchors_no_anchor() {
    shot_bytecode!(
        r#"
        Test = (array (identifier) (number))
    "#
    );
}

#[test]
fn anchors_explicit_comment_pattern_emits_match_before_skip_nav() {
    shot_bytecode!(
        r#"
        Test = (program {(comment) @c . (function_declaration) @f})
    "#
    );
}

#[test]
fn definitions_single() {
    shot_bytecode!(
        r#"
        Foo = (identifier) @id
    "#
    );
}

#[test]
fn definitions_multiple() {
    shot_bytecode!(
        r#"
        Foo = (identifier) @id
        Bar = (string) @str
    "#
    );
}

#[test]
fn definitions_reference() {
    shot_bytecode!(
        r#"
        Expression = [(identifier) @name (number) @value]
        Root = (function_declaration name: (identifier) @name)
    "#
    );
}

#[test]
fn definitions_nested_capture() {
    shot_bytecode!(
        r#"
        Inner = (call_expression (identifier) @name)
        Outer = (array {(Inner) @item}* @items)
    "#
    );
}

#[test]
fn recursion_simple() {
    shot_bytecode!(
        r#"
        Expr = [
            Lit: (number) @value :: string
            Rec: (call_expression function: (identifier) @fn arguments: (Expr) @inner)
        ]
    "#
    );
}

#[test]
fn recursion_with_structured_result() {
    shot_bytecode!(
        r#"
        Expr = [
          Lit: (number) @value :: string
          Nested: (call_expression function: (identifier) @fn arguments: (Expr) @inner)
        ]

        Test = (program (Expr) @expr)
    "#
    );
}

#[test]
fn recursion_def_level_alias_emits_alias_type() {
    shot_bytecode!(
        r#"
        Rec = [A: (program (expression_statement (Rec) @inner)) B: (identifier) @id]
        Q = (Rec)
    "#
    );
}

#[test]
fn optional_first_child() {
    shot_bytecode!(
        r#"
        Test = (program (lexical_declaration)? @id (debugger_statement) @n)
    "#
    );
}

#[test]
fn optional_first_child_then_interior_anchor() {
    // Regression for #415: the interior anchor must survive into both follower
    // compilations. The match path (optional present) gets a NextSkip (`─•─`)
    // and the skip path (optional absent) a DownSkip (`└•─`) — both bounded, not
    // the unanchored Next/Down (`‣`) that silently searched past named nodes.
    shot_bytecode!(
        r#"
        Test = (program {(lexical_declaration)? @a . (debugger_statement) @b})
    "#
    );
}

#[test]
fn optional_first_child_then_strict_interior_anchor() {
    // As above, but the strict anchor must degrade to NextExact / DownExact on the
    // match and skip paths respectively.
    shot_bytecode!(
        r#"
        Test = (program {(lexical_declaration)? @a .! (debugger_statement) @b})
    "#
    );
}

#[test]
fn optional_null_injection() {
    shot_bytecode!(
        r#"
        Test = (function_declaration (decorator)? @dec)
    "#
    );
}

#[test]
fn alternation_branches_share_head_node() {
    shot_bytecode!(
        r#"
        Test = [(object (pair)) (object (spread_element))]
    "#
    );
}

#[test]
fn comprehensive_multi_definition() {
    shot_bytecode!(
        r#"
        Ident = (identifier) @name :: string
        Expression = [
            Literal: (number) @value
            Variable: (identifier) @name
        ]
        Assignment = (assignment_expression
            left: (identifier) @target
            right: (Expression) @value)
    "#
    );
}
