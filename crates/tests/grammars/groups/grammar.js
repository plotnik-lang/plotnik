// Generate with the stock tree-sitter CLI 0.26.10: tree-sitter generate.
module.exports = grammar({
  name: "groups",

  extras: ($) => [/\s/, $.comment__line, $.comment__block],

  rules: {
    program: ($) =>
      repeat(
        choice(
          $.function__regular,
          $.function__arrow,
          $.plain,
          $.pair,
          $.value__leaf,
          $.value__group,
        ),
      ),

    function__regular: ($) =>
      seq(
        "fn",
        field("name", $.identifier),
        "(",
        optional(field("parameter", $.identifier)),
        ")",
        field("body", $.block),
      ),
    function__arrow: ($) =>
      seq(
        "arrow",
        "(",
        optional(field("parameter", $.identifier)),
        ")",
        "=>",
        field("value", $.identifier),
        ";",
      ),
    block: ($) => seq("{", repeat(seq($.identifier, optional(","))), "}"),
    plain: ($) => seq("plain", field("label__name", $.identifier), ";"),
    pair: ($) =>
      seq(
        "pair",
        "{",
        field("left", choice($.function__regular, $.function__arrow)),
        field("right", choice($.function__regular, $.function__arrow)),
        "}",
      ),

    value__leaf: () => "unit",
    value__group: ($) => seq("group", "(", $.identifier, ")"),

    identifier: () => /[a-z][a-z0-9_]*/,
    comment__line: () => token(seq("//", /[^\n]*/)),
    comment__block: () => token(seq("/*", /[^*]*\*+([^/*][^*]*\*+)*/, "/")),
  },
});
