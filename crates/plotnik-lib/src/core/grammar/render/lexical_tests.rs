use super::super::rules::{Precedence, Rule};
use super::TokenText;
use super::lexical::synthesize;

fn regex(rule: &Rule) -> String {
    match synthesize(rule) {
        TokenText::Regex(value) => value,
        TokenText::Str(value) => panic!("expected regex, got string {value:?}"),
    }
}

fn string(rule: &Rule) -> String {
    match synthesize(rule) {
        TokenText::Str(value) => value,
        TokenText::Regex(value) => panic!("expected string, got regex {value:?}"),
    }
}

#[test]
fn plain_string_is_a_literal() {
    assert_eq!(string(&Rule::String("true".to_string())), "true");
}

#[test]
fn single_member_seq_collapses_to_literal() {
    let rule = Rule::Seq(vec![Rule::String("null".to_string())]);
    assert_eq!(string(&rule), "null");
}

#[test]
fn string_metacharacters_are_escaped() {
    // A string seq forces regex synthesis; metacharacters and the `/` delimiter escape.
    let rule = Rule::Seq(vec![
        Rule::String("a.".to_string()),
        Rule::String("/".to_string()),
    ]);
    assert_eq!(regex(&rule), "a\\.\\/");
}

#[test]
fn pattern_slashes_are_escaped_but_classes_pass_through() {
    let rule = Rule::Pattern("[^/*]".to_string(), String::new());
    assert_eq!(regex(&rule), "[^\\/*]");
}

#[test]
fn choice_becomes_alternation() {
    let rule = Rule::Choice(vec![
        Rule::String("//".to_string()),
        Rule::String("/*".to_string()),
    ]);
    assert_eq!(regex(&rule), "\\/\\/|\\/\\*");
}

#[test]
fn alternation_inside_seq_is_parenthesized() {
    let rule = Rule::Seq(vec![
        Rule::String("a".to_string()),
        Rule::Choice(vec![
            Rule::String("b".to_string()),
            Rule::String("c".to_string()),
        ]),
    ]);
    assert_eq!(regex(&rule), "a(b|c)");
}

#[test]
fn choice_with_blank_is_optional() {
    // `-` is literal outside a character class, so it is not escaped.
    let rule = Rule::Choice(vec![Rule::String("-".to_string()), Rule::Blank]);
    assert_eq!(regex(&rule), "-?");
}

#[test]
fn multi_char_optional_is_grouped() {
    let rule = Rule::Choice(vec![Rule::String("ab".to_string()), Rule::Blank]);
    assert_eq!(regex(&rule), "(ab)?");
}

#[test]
fn repeat_with_blank_is_star() {
    let rule = Rule::Choice(vec![
        Rule::Repeat(Box::new(Rule::Pattern("\\d".to_string(), String::new()))),
        Rule::Blank,
    ]);
    assert_eq!(regex(&rule), "\\d*");
}

#[test]
fn repeat1_is_plus() {
    let rule = Rule::Repeat(Box::new(Rule::Pattern("\\d".to_string(), String::new())));
    assert_eq!(regex(&rule), "\\d+");
}

#[test]
fn repeat_over_concatenation_is_grouped() {
    let rule = Rule::Repeat(Box::new(Rule::Seq(vec![
        Rule::String("a".to_string()),
        Rule::String("b".to_string()),
    ])));
    assert_eq!(regex(&rule), "(ab)+");
}

#[test]
fn precedence_and_token_metadata_are_transparent() {
    let rule = Rule::prec(
        Precedence::Integer(1),
        Rule::token(Rule::Pattern("[a-z]+".to_string(), String::new())),
    );
    assert_eq!(regex(&rule), "[a-z]+");
}

#[test]
fn case_insensitive_flag_becomes_inline_group() {
    let rule = Rule::Pattern("abc".to_string(), "i".to_string());
    assert_eq!(regex(&rule), "(?i:abc)");
}
