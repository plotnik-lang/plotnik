; Node kinds and field names
(named_node kind: (identifier) @tag)
(named_node subtype: (identifier) @tag)
(missing_node kind: (identifier) @tag)
(field name: (identifier) @property)
(negated_field name: (identifier) @property)

; Definitions, references, branch labels, type names
(type_identifier) @type
(capture_type_identifier) @type
(builtin_capture_type) @type.builtin

; Captures
(capture) @label
(suppressive_capture) @label

; Literals
(string) @string
(regex) @string.special
(wildcard) @constant.builtin

; Keywords
[
  "ERROR"
  "MISSING"
] @keyword

; Operators
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

; Punctuation
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
(shebang) @comment
