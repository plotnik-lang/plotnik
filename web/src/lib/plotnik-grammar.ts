import type { LanguageRegistration } from "shiki";

/**
 * Deliberately lexical TextMate grammar for plotnik query syntax.
 * Token-level only (no structural nesting): captures, node kinds,
 * PascalCase defs/refs, fields, strings, predicates, quantifiers.
 * The tree-sitter grammar (separate repo) owns editor/playground
 * highlighting; this one feeds Shiki (site + docs) and, later,
 * GitHub linguist. Keep them in sync at the token level only.
 */
export const plotnikGrammar: LanguageRegistration = {
  name: "plotnik",
  scopeName: "source.plotnik",
  fileTypes: ["ptk"],
  patterns: [
    { include: "#comment" },
    { include: "#shebang" },
    { include: "#definition" },
    { include: "#capture" },
    { include: "#capture-type" },
    { include: "#field" },
    { include: "#negated-field" },
    { include: "#node-ref" },
    { include: "#node-kind" },
    { include: "#enum-tag" },
    { include: "#string" },
    { include: "#regex" },
    { include: "#predicate-op" },
    { include: "#anchor" },
    { include: "#quantifier" },
    { include: "#wildcard" },
    { include: "#punctuation" },
  ],
  repository: {
    comment: {
      patterns: [
        { name: "comment.line.double-slash.plotnik", match: "//.*$" },
        { name: "comment.block.plotnik", begin: "/\\*", end: "\\*/" },
      ],
    },
    shebang: {
      name: "comment.line.shebang.plotnik",
      match: "^#!.*$",
    },
    definition: {
      // `Name =` at a definition site
      match: "\\b([A-Z][A-Za-z0-9_]*)\\s*(=)(?!=|~)",
      captures: {
        "1": { name: "entity.name.type.definition.plotnik" },
        "2": { name: "keyword.operator.assignment.plotnik" },
      },
    },
    "enum-tag": {
      // `Tag:` inside [...] enum branches
      match: "\\b([A-Z][A-Za-z0-9_]*)(:)",
      captures: {
        "1": { name: "entity.name.type.variant.plotnik" },
        "2": { name: "punctuation.separator.plotnik" },
      },
    },
    capture: {
      name: "variable.capture.plotnik",
      match: "@[a-z_][a-z0-9_]*\\b|@_",
    },
    "capture-type": {
      match: "(::)\\s*([a-z][A-Za-z0-9_]*|[A-Z][A-Za-z0-9]*)\\b",
      captures: {
        "1": { name: "keyword.operator.type.plotnik" },
        "2": { name: "entity.name.type.capture.plotnik" },
      },
    },
    field: {
      match: "\\b([a-z_][a-z0-9_]*)(:)",
      captures: {
        "1": { name: "variable.other.member.plotnik" },
        "2": { name: "punctuation.separator.plotnik" },
      },
    },
    "negated-field": {
      match: "(-)([a-z_][a-z0-9_]*)\\b",
      captures: {
        "1": { name: "keyword.operator.negation.plotnik" },
        "2": { name: "variable.other.member.plotnik" },
      },
    },
    "node-ref": {
      // `(Name)` — reference to a named definition
      match: "(?<=\\()\\s*([A-Z][A-Za-z0-9_]*)\\b",
      captures: {
        "1": { name: "entity.name.type.reference.plotnik" },
      },
    },
    "node-kind": {
      // `(node_kind` — tree-sitter named node
      match: "(?<=\\()\\s*([a-z_][a-z0-9_]*)\\b",
      captures: {
        "1": { name: "entity.name.tag.plotnik" },
      },
    },
    string: {
      patterns: [
        {
          name: "string.quoted.double.plotnik",
          begin: '"',
          end: '"',
          patterns: [
            { name: "constant.character.escape.plotnik", match: "\\\\." },
          ],
        },
        {
          name: "string.quoted.single.plotnik",
          begin: "'",
          end: "'",
          patterns: [
            { name: "constant.character.escape.plotnik", match: "\\\\." },
          ],
        },
      ],
    },
    regex: {
      name: "string.regexp.plotnik",
      match: "/(?:[^/\\\\\\n]|\\\\.)+/",
    },
    "predicate-op": {
      name: "keyword.operator.comparison.plotnik",
      match: "==|!=|\\^=|\\$=|\\*=|=~|!~",
    },
    anchor: {
      name: "keyword.operator.anchor.plotnik",
      match: "\\.!|\\.(?![a-zA-Z0-9])",
    },
    quantifier: {
      name: "keyword.operator.quantifier.plotnik",
      match: "[?*+]\\??",
    },
    wildcard: {
      name: "constant.language.wildcard.plotnik",
      match: "\\b_\\b",
    },
    punctuation: {
      name: "punctuation.bracket.plotnik",
      match: "[(){}\\[\\]]",
    },
  },
};
