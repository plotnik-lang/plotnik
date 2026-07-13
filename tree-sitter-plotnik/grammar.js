/**
 * Tree-sitter grammar for the Plotnik query language (.ptk files).
 *
 * Mirrors the reference implementation in
 * `crates/plotnik-lib/src/compiler/parse/` at the syntax level: what the
 * reference parser accepts without error-level diagnostics parses cleanly
 * here, while analyze/link-stage rejections (unknown node kinds, empty
 * trees, dimensionality, ...) are out of scope, as is usual for editor
 * grammars. See README.md for the exact contract and known divergences.
 */

const QUANTIFIERS = ["*", "+", "?", "*?", "+?", "??"];
const STRING_OPS = ["==", "!=", "^=", "$=", "*="];
const REGEX_OPS = ["=~", "!~"];

module.exports = grammar({
  name: "plotnik",

  // No `\s`: the reference lexer only knows spaces, tabs, `\n` and `\r\n`;
  // anything else (lone `\r`, form feed, NBSP) is an error there too.
  extras: ($) => [/[ \t]+/, /\r?\n/, $.comment],

  rules: {
    // A .ptk module is a list of named definitions. Bare patterns are a
    // CLI-only script form (`-q`) and are rejected in module files.
    source_file: ($) => seq(optional($.shebang), repeat($.definition)),

    definition: ($) =>
      seq(field("name", $.type_identifier), "=", field("body", $._pattern)),

    // A pattern with optional quantifier/capture suffixes. Wrappers nest
    // the way the reference CST does:
    //   `(x)* @a` = (captured_pattern (quantified_pattern (named_node)))
    // A quantifier binds tighter than a capture, and each may appear once.
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

    // Capture types attach to regular captures only; `@_ :: T` is an
    // error in the reference parser.
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

    builtin_capture_type: (_) => choice("str", "bool"),

    // Lowercase names are syntactically complete so semantic analysis can
    // diagnose unknown built-ins. Custom names are PascalCase and deliberately
    // exclude `_`, matching the reference parser's validation boundary.
    capture_type_identifier: (_) =>
      choice(/[a-z][a-zA-Z0-9_]*/, /[A-Z][a-zA-Z0-9]*/),

    // What may appear among a node's children. Anchors and negated fields
    // are positional assertions, not patterns: they never take suffixes and
    // are not valid as definition bodies, field values, or alternatives. A
    // sequence additionally admits anchors but not negated fields — those
    // constrain the enclosing node and must be its direct children.
    _child: ($) => choice($._pattern, $.anchor, $.negated_field),

    _sequence_item: ($) => choice($._pattern, $.anchor),

    // `(kind ...)`, `(_ ...)`, or the empty tree `()`. A supertype
    // refinement or an inline predicate is only valid after a concrete
    // node kind, in that order, before any children.
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

    // `(supertype#subtype)`, bare `(supertype#)`, or the deprecated
    // tree-sitter spelling `(supertype/subtype)` (which requires a
    // subtype). Both separator and subtype are tight-binding: whitespace
    // around them changes the meaning, exactly as in tree-sitter.
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

    // Inline node predicate: `(identifier == "foo")`, `(name =~ /^get/)`.
    // String operators take strings and regex operators take regexes;
    // mismatched combinations never compile in the reference pipeline.
    predicate: ($) =>
      choice(
        seq(field("op", choice(...STRING_OPS)), field("value", $.string)),
        seq(field("op", choice(...REGEX_OPS)), field("value", $.regex)),
      ),

    // `(Name)` references a named definition. The PascalCase head is what
    // separates a reference from a node kind; references take no children.
    def_ref: ($) => seq("(", field("name", $.type_identifier), ")"),

    error_node: (_) => seq("(", "ERROR", ")"),

    // `(MISSING)`, `(MISSING kind)`, or `(MISSING ";")`. A missing node is
    // a zero-byte node inserted by error recovery, so the reference
    // parser rejects children — only the optional kind argument is legal.
    missing_node: ($) =>
      seq(
        "(",
        "MISSING",
        optional(
          field("kind", choice($.identifier, $.type_identifier, $.string)),
        ),
        ")",
      ),

    // `{...}`: siblings in order. Grouping only; never a field value's
    // shape requirement or an output scope unless captured.
    sequence: ($) => seq("{", repeat($._sequence_item), "}"),

    // `[...]`: alternatives tried in source order with backtracking. They are
    // either all labeled or all unlabeled; that distinction is semantic.
    alternation: ($) => seq("[", repeat($.alternative), "]"),

    // An alternative body is a pattern; anchors and negated fields cannot form
    // alternatives (labeled or not). An unlabeled alternative must not start with
    // `name:` — a lowercase label is how the reference parser reads that,
    // and it rejects it — so the unlabeled arm uses a field-free copy of
    // the pattern rules.
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

    // `name: pattern`. The value never carries a suffix — a quantifier or
    // capture after it wraps the whole field constraint — and it can be
    // another field. Anchors and negated fields are excluded: they are
    // never meaningful as a field's value.
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

    // `-field`: assert the field is absent.
    negated_field: ($) => seq("-", field("name", $.identifier)),

    // `.` is soft adjacency, `.!` is exact adjacency.
    anchor: (_) => choice(".", ".!"),

    // `_` matches any node; `(_)` any named node. The same token serves as
    // a bare pattern and as a parenthesized node's kind.
    wildcard: (_) => "_",

    // Anonymous node / literal text. Single and double quotes are
    // equivalent. Content may span newlines; escapes cannot. The content
    // token outranks comments lexically, or `";"` would lex its body as a
    // line comment — the reference lexer never has token boundaries
    // inside a string.
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

    // `/pattern/`, only after `=~` / `!~`. One token: the reference lexer
    // consumes everything to the closing unescaped slash on the same line,
    // comment-lookalikes included.
    regex: (_) => /\/([^\/\\\n]|\\[^\n])*\//,

    // Node kinds and field names. Dots and hyphens are tolerated by the
    // reference lexer for tree-sitter compatibility.
    identifier: (_) => /[a-z][a-zA-Z0-9_.\-]*/,

    // Definition names, references, alternative labels, and type names. The
    // leading uppercase letter alone decides identifier intent.
    type_identifier: (_) => /[A-Z][a-zA-Z0-9_]*/,

    capture: (_) => /@[a-z][a-z0-9_]*/,

    // `@_` or `@_name`: match, then discard the output.
    discard: (_) => /@_[a-z0-9_]*/,

    // `; line`, `// line`, and non-nesting `/* block */`. The block form
    // mirrors the reference regex exactly, quirks included: a body `*`
    // must not be followed by `/` — so `/* x **/` is not a comment there,
    // and is not one here.
    comment: (_) =>
      token(choice(/\/\/[^\n]*/, /;[^\n]*/, /\/\*([^*]|\*[^\/])*\*\//)),

    // Only meaningful on the first line; elsewhere `#!` is garbage. The
    // grammar approximates "first line" as "before any definition".
    shebang: (_) => /#![^\n]*/,
  },
});
