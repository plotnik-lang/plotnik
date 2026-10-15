/**
 * Tree-sitter grammar for the Plotnik query language (.ptk files).
 *
 * Mirrors the reference implementation in
 * `crates/compiler/src/compiler/parse/` at the syntax level: what the
 * reference parser accepts without error-level diagnostics parses cleanly
 * here, while analyze/bind-stage rejections (unknown node kinds, empty
 * trees, dimensionality, ...) are out of scope, as is usual for editor
 * grammars. See README.md for the exact contract and known divergences.
 */

const QUANTIFIERS = ["*", "+", "?", "*?", "+?", "??"];
const STRING_OPS = ["==", "!=", "^=", "$=", "*="];
const REGEX_OPS = ["=~", "!~"];

module.exports = grammar({
  name: "plotnik",

  extras: ($) => [/[ \t]+/, /\r?\n/, $.comment],

  rules: {
    source_file: ($) => repeat($.definition),

    definition: ($) =>
      seq(field("name", $.type_identifier), "=", field("body", $._pattern)),

    _pattern: ($) =>
      choice($._suffixable, $.quantified_pattern, $.captured_pattern),

    _suffixable: ($) =>
      choice(
        $.named_node,
        $.def_ref,
        $.error_node,
        $.missing_node,
        $.string,
        $.wildcard,
        $.sequence,
        $.alternation,
        $.field,
      ),

    quantified_pattern: ($) =>
      seq(field("pattern", $._suffixable), field("quantifier", $.quantifier)),

    quantifier: (_) => choice(...QUANTIFIERS),

    captured_pattern: ($) =>
      seq(
        field("pattern", choice($._suffixable, $.quantified_pattern)),
        choice(
          seq(field("capture", $.capture), optional($.capture_type)),
          field("capture", $.discard),
        ),
      ),

    capture_type: ($) =>
      seq(
        "::",
        field(
          "type",
          choice($.builtin_capture_type, $.capture_type_identifier),
        ),
      ),

    builtin_capture_type: (_) => choice("text", "bool"),

    capture_type_identifier: (_) =>
      choice(/[a-z][a-zA-Z0-9_]*/, /[A-Z][a-zA-Z0-9]*/),

    _child: ($) => choice($._pattern, $.anchor, $.negated_field),

    _sequence_item: ($) => choice($._pattern, $.anchor),

    named_node: ($) =>
      choice(
        seq("(", ")"),
        seq("(", field("kind", $.wildcard), repeat($._child), ")"),
        seq(
          "(",
          field("kind", $.identifier),
          optional($._refinement),
          optional($.predicate),
          repeat($._child),
          ")",
        ),
      ),

    _refinement: ($) =>
      choice(
        seq(token.immediate("#"), optional(field("subtype", $._subtype))),
        seq(token.immediate("/"), field("subtype", $._subtype)),
      ),

    _subtype: ($) =>
      choice(
        alias(token.immediate(/[a-zA-Z][a-zA-Z0-9_.\-]*/), $.identifier),
        alias(token.immediate(/"([^"\\]|\\.)*"|'([^'\\]|\\.)*'/), $.string),
      ),

    predicate: ($) =>
      choice(
        seq(field("op", choice(...STRING_OPS)), field("value", $.string)),
        seq(field("op", choice(...REGEX_OPS)), field("value", $.regex)),
      ),

    def_ref: ($) => seq("(", field("name", $.type_identifier), ")"),

    error_node: (_) => seq("(", "ERROR", ")"),

    missing_node: ($) =>
      seq(
        "(",
        "MISSING",
        optional(
          field("kind", choice($.identifier, $.type_identifier, $.string)),
        ),
        ")",
      ),

    sequence: ($) => seq("{", repeat($._sequence_item), "}"),

    alternation: ($) => seq("[", repeat($.alternative), "]"),

    alternative: ($) =>
      choice(
        seq(field("label", $.type_identifier), ":", field("body", $._pattern)),
        field(
          "body",
          choice(
            $._alternative_suffixable,
            alias($._alternative_quantified, $.quantified_pattern),
            alias($._alternative_captured, $.captured_pattern),
          ),
        ),
      ),

    _alternative_suffixable: ($) =>
      choice(
        $.named_node,
        $.def_ref,
        $.error_node,
        $.missing_node,
        $.string,
        $.wildcard,
        $.sequence,
        $.alternation,
      ),

    _alternative_quantified: ($) =>
      seq(
        field("pattern", $._alternative_suffixable),
        field("quantifier", $.quantifier),
      ),

    _alternative_captured: ($) =>
      seq(
        field(
          "pattern",
          choice(
            $._alternative_suffixable,
            alias($._alternative_quantified, $.quantified_pattern),
          ),
        ),
        choice(
          seq(field("capture", $.capture), optional($.capture_type)),
          field("capture", $.discard),
        ),
      ),

    field: ($) =>
      seq(field("name", $.identifier), ":", field("value", $._field_value)),

    _field_value: ($) =>
      choice(
        $.named_node,
        $.def_ref,
        $.error_node,
        $.missing_node,
        $.string,
        $.wildcard,
        $.sequence,
        $.alternation,
        $.field,
      ),

    negated_field: ($) => seq("-", field("name", $.identifier)),

    anchor: (_) => choice(".", ".!"),

    wildcard: (_) => "_",

    string: ($) =>
      choice(
        seq(
          '"',
          optional(
            alias(token.immediate(prec(1, /([^"\\]|\\.)+/)), $.string_content),
          ),
          token.immediate('"'),
        ),
        seq(
          "'",
          optional(
            alias(token.immediate(prec(1, /([^'\\]|\\.)+/)), $.string_content),
          ),
          token.immediate("'"),
        ),
      ),

    regex: (_) => /\/([^\/\\\n]|\\[^\n])*\//,

    identifier: (_) => /[a-z][a-zA-Z0-9_.\-]*/,

    type_identifier: (_) => /[A-Z][a-zA-Z0-9_]*/,

    capture: (_) => /@[a-z][a-z0-9_]*/,

    discard: (_) => /@_[a-z0-9_]*/,

    comment: (_) =>
      token(choice(/\/\/[^\n]*/, /;[^\n]*/, /\/\*([^*]|\*[^\/])*\*\//)),
  },
});
