(named_node kind: (identifier) @tag)
(named_node subtype: (identifier) @tag)
(missing_node kind: (identifier) @tag)
(field name: (identifier) @property)
(negated_field name: (identifier) @property)

(type_identifier) @type
(capture_type_identifier) @type
(builtin_capture_type) @type.builtin

(capture) @label
(discard) @label

(string) @string
(regex) @string.special
(wildcard) @constant.builtin

[
  "ERROR"
  "MISSING"
] @keyword

(quantifier) @operator
(anchor) @operator
[
  "=="
  "!="
  "^="
  "$="
  "*="
  "=~"
  "!~"
] @operator
"-" @operator

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket
[
  ":"
  "::"
  "="
] @punctuation.delimiter

(comment) @comment
