//! Test utilities.

use std::sync::LazyLock;

use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::core::grammar::{Grammar, raw::RawGrammar};
use indexmap::IndexMap;
use indoc::indoc;

pub fn empty_symbol_table() -> SymbolTable {
    SymbolTable::new(IndexMap::new(), IndexMap::new())
}

pub fn empty_dependency_analysis() -> DependencyAnalysis {
    DependencyAnalysis::empty()
}

pub fn synthetic_grammar() -> &'static Grammar {
    static GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| {
        let raw = RawGrammar::from_json(indoc! {r##"
            {
              "name": "plotnik_synthetic",
              "extras": [
                { "type": "SYMBOL", "name": "comment" }
              ],
              "rules": {
                "program": {
                  "type": "REPEAT",
                  "content": { "type": "SYMBOL", "name": "_statement" }
                },
                "_statement": {
                  "type": "CHOICE",
                  "members": [
                    { "type": "SYMBOL", "name": "expression_statement" },
                    { "type": "SYMBOL", "name": "lexical_declaration" },
                    { "type": "SYMBOL", "name": "function_declaration" },
                    { "type": "SYMBOL", "name": "class_declaration" },
                    { "type": "SYMBOL", "name": "statement_block" },
                    { "type": "SYMBOL", "name": "comment" }
                  ]
                },
                "_expression": {
                  "type": "CHOICE",
                  "members": [
                    { "type": "SYMBOL", "name": "identifier" },
                    { "type": "SYMBOL", "name": "number" },
                    { "type": "SYMBOL", "name": "string" },
                    { "type": "SYMBOL", "name": "array" },
                    { "type": "SYMBOL", "name": "call_expression" },
                    { "type": "SYMBOL", "name": "member_expression" },
                    { "type": "SYMBOL", "name": "binary_expression" },
                    { "type": "SYMBOL", "name": "unary_expression" }
                  ]
                },
                "identifier": { "type": "PATTERN", "value": "[a-zA-Z_$][a-zA-Z0-9_$]*" },
                "number": { "type": "PATTERN", "value": "[0-9]+" },
                "string": { "type": "PATTERN", "value": "\"[^\"]*\"" },
                "comment": { "type": "PATTERN", "value": "//[^\\n]*" },
                "expression_statement": {
                  "type": "SYMBOL",
                  "name": "_expression"
                },
                "lexical_declaration": {
                  "type": "SYMBOL",
                  "name": "variable_declarator"
                },
                "variable_declarator": {
                  "type": "SEQ",
                  "members": [
                    {
                      "type": "FIELD",
                      "name": "name",
                      "content": { "type": "SYMBOL", "name": "identifier" }
                    },
                    {
                      "type": "CHOICE",
                      "members": [
                        {
                          "type": "FIELD",
                          "name": "value",
                          "content": { "type": "SYMBOL", "name": "_expression" }
                        },
                        { "type": "BLANK" }
                      ]
                    }
                  ]
                },
                "array": {
                  "type": "SEQ",
                  "members": [
                    { "type": "STRING", "value": "[" },
                    {
                      "type": "REPEAT",
                      "content": { "type": "SYMBOL", "name": "_expression" }
                    },
                    { "type": "STRING", "value": "]" }
                  ]
                },
                "arguments": {
                  "type": "SEQ",
                  "members": [
                    { "type": "STRING", "value": "(" },
                    {
                      "type": "REPEAT",
                      "content": { "type": "SYMBOL", "name": "_expression" }
                    },
                    { "type": "STRING", "value": ")" }
                  ]
                },
                "optional_chain": { "type": "STRING", "value": "?." },
                "call_expression": {
                  "type": "SEQ",
                  "members": [
                    {
                      "type": "FIELD",
                      "name": "function",
                      "content": {
                        "type": "CHOICE",
                        "members": [
                          { "type": "SYMBOL", "name": "identifier" },
                          { "type": "SYMBOL", "name": "member_expression" }
                        ]
                      }
                    },
                    {
                      "type": "FIELD",
                      "name": "optional_chain",
                      "content": { "type": "SYMBOL", "name": "optional_chain" }
                    },
                    {
                      "type": "FIELD",
                      "name": "arguments",
                      "content": { "type": "SYMBOL", "name": "arguments" }
                    }
                  ]
                },
                "member_expression": {
                  "type": "SEQ",
                  "members": [
                    {
                      "type": "FIELD",
                      "name": "object",
                      "content": { "type": "SYMBOL", "name": "identifier" }
                    },
                    {
                      "type": "FIELD",
                      "name": "property",
                      "content": { "type": "SYMBOL", "name": "identifier" }
                    }
                  ]
                },
                "binary_expression": {
                  "type": "SEQ",
                  "members": [
                    {
                      "type": "FIELD",
                      "name": "left",
                      "content": { "type": "SYMBOL", "name": "_expression" }
                    },
                    {
                      "type": "FIELD",
                      "name": "right",
                      "content": { "type": "SYMBOL", "name": "_expression" }
                    }
                  ]
                },
                "unary_expression": {
                  "type": "SEQ",
                  "members": [
                    { "type": "STRING", "value": "!" },
                    {
                      "type": "FIELD",
                      "name": "argument",
                      "content": { "type": "SYMBOL", "name": "_expression" }
                    }
                  ]
                },
                "formal_parameters": {
                  "type": "SEQ",
                  "members": [
                    { "type": "STRING", "value": "(" },
                    {
                      "type": "REPEAT",
                      "content": { "type": "SYMBOL", "name": "identifier" }
                    },
                    { "type": "STRING", "value": ")" }
                  ]
                },
                "statement_block": {
                  "type": "SEQ",
                  "members": [
                    { "type": "STRING", "value": "{" },
                    {
                      "type": "REPEAT",
                      "content": { "type": "SYMBOL", "name": "_statement" }
                    },
                    { "type": "STRING", "value": "}" }
                  ]
                },
                "function_declaration": {
                  "type": "SEQ",
                  "members": [
                    {
                      "type": "FIELD",
                      "name": "name",
                      "content": { "type": "SYMBOL", "name": "identifier" }
                    },
                    {
                      "type": "FIELD",
                      "name": "parameters",
                      "content": { "type": "SYMBOL", "name": "formal_parameters" }
                    },
                    {
                      "type": "FIELD",
                      "name": "body",
                      "content": { "type": "SYMBOL", "name": "statement_block" }
                    }
                  ]
                },
                "class_declaration": {
                  "type": "SEQ",
                  "members": [
                    {
                      "type": "FIELD",
                      "name": "name",
                      "content": { "type": "SYMBOL", "name": "identifier" }
                    },
                    {
                      "type": "FIELD",
                      "name": "body",
                      "content": { "type": "SYMBOL", "name": "statement_block" }
                    }
                  ]
                }
              }
            }
        "##})
        .expect("synthetic grammar fixture");
        Grammar::from_raw(&raw).expect("synthetic grammar metadata")
    });

    &GRAMMAR
}

pub fn colliding_node_kind_grammar() -> Grammar {
    let raw = RawGrammar::from_json(
        r#"{
        "name": "collision",
        "rules": {
            "program": {
                "type": "CHOICE",
                "members": [
                    { "type": "SYMBOL", "name": "number" },
                    { "type": "STRING", "value": "number" }
                ]
            },
            "number": { "type": "STRING", "value": "literal" }
        }
    }"#,
    )
    .expect("collision grammar fixture");

    Grammar::from_raw(&raw).expect("collision grammar metadata")
}
