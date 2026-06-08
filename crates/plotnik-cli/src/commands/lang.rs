use std::collections::HashSet;
use std::process::exit;

use plotnik_core::grammar::raw::{RawGrammar, RawRule};

use super::language_registry;

/// List all supported languages with aliases.
pub fn run_list() {
    for lang in language_registry::all() {
        let aliases: Vec<_> = lang.aliases().iter().skip(1).copied().collect();
        if aliases.is_empty() {
            println!("{}", lang.name());
        } else {
            println!("{} ({})", lang.name(), aliases.join(", "));
        }
    }
}

/// Dump grammar for a language.
pub fn run_dump(lang_name: &str) {
    let Some(lang) = language_registry::from_name(lang_name) else {
        eprintln!("error: unknown language '{lang_name}'");
        eprintln!();
        eprintln!("Run 'plotnik lang list' to see available languages.");
        exit(1);
    };

    let renderer = GrammarRenderer::new(lang.raw());
    print!("{}", renderer.render());
}

pub struct GrammarRenderer<'a> {
    grammar: &'a RawGrammar,
    hidden_rules: HashSet<&'a str>,
}

impl<'a> GrammarRenderer<'a> {
    pub fn new(grammar: &'a RawGrammar) -> Self {
        let hidden_rules: HashSet<_> = grammar
            .rules
            .iter()
            .filter(|(name, _)| name.starts_with('_'))
            .map(|(name, _)| name.as_str())
            .collect();

        Self {
            grammar,
            hidden_rules,
        }
    }

    pub fn render(&self) -> String {
        let mut out = String::new();

        self.render_header(&mut out);
        self.render_extras(&mut out);
        self.render_externals(&mut out);
        self.render_supertypes(&mut out);
        self.render_rules(&mut out);

        out
    }

    fn render_header(&self, out: &mut String) {
        out.push_str(
            r#"/*
 * Grammar Dump
 *
 * Syntax:
 *   (node_kind)        named node (queryable)
 *   "literal"          anonymous node (queryable)
 *   (_hidden ...)      hidden rule (not queryable, children inline)
 *   {...}              sequence (ordered children)
 *   [...]              alternation (first match)
 *   ?  *  +            quantifiers (0-1, 0+, 1+)
 *   "x"!               immediate token (no preceding whitespace)
 *   field: ...         named field
 *   T :: supertype     supertype declaration
 */

"#,
        );
    }

    fn render_extras(&self, out: &mut String) {
        self.render_rule_list("extras", &self.grammar.extras, out);
    }

    fn render_externals(&self, out: &mut String) {
        self.render_rule_list("externals", &self.grammar.externals, out);
    }

    fn render_rule_list(&self, label: &str, rules: &[RawRule], out: &mut String) {
        if rules.is_empty() {
            return;
        }

        out.push_str(label);
        out.push_str(" = [\n");
        for rule in rules {
            out.push_str("  ");
            self.render_rule(rule, out, 1);
            out.push('\n');
        }
        out.push_str("]\n\n");
    }

    fn render_supertypes(&self, out: &mut String) {
        for supertype in &self.grammar.supertypes {
            if let Some(rule) = self.grammar.rules.get(supertype) {
                out.push_str(supertype);
                out.push_str(" :: supertype = ");
                self.render_rule(rule, out, 0);
                out.push_str("\n\n");
            }
        }
    }

    fn render_rules(&self, out: &mut String) {
        let supertypes_set: HashSet<_> =
            self.grammar.supertypes.iter().map(String::as_str).collect();

        for (name, rule) in &self.grammar.rules {
            if supertypes_set.contains(name.as_str()) {
                continue;
            }

            out.push_str(name);
            out.push_str(" = ");
            self.render_rule(rule, out, 0);
            out.push_str("\n\n");
        }
    }

    fn render_rule(&self, rule: &RawRule, out: &mut String, indent: usize) {
        match rule {
            RawRule::BLANK => out.push_str("()"),

            RawRule::STRING { value } => {
                out.push('"');
                for c in value.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        _ => out.push(c),
                    }
                }
                out.push('"');
            }

            RawRule::PATTERN { value, flags } => {
                out.push('/');
                out.push_str(value);
                out.push('/');
                if let Some(f) = flags {
                    out.push_str(f);
                }
            }

            RawRule::SYMBOL { name } => {
                out.push('(');
                out.push_str(name);
                if self.hidden_rules.contains(name.as_str()) {
                    out.push_str(" ...)");
                } else {
                    out.push(')');
                }
            }

            RawRule::SEQ { members } => {
                self.render_block(members, '{', '}', indent, out);
            }

            RawRule::CHOICE { members } => {
                if let Some(simplified) = self.simplify_optional(members) {
                    self.render_rule(&simplified, out, indent);
                    out.push('?');
                } else {
                    self.render_block(members, '[', ']', indent, out);
                }
            }

            RawRule::REPEAT { content } => {
                self.render_rule(content, out, indent);
                out.push('*');
            }

            RawRule::REPEAT1 { content } => {
                self.render_rule(content, out, indent);
                out.push('+');
            }

            RawRule::FIELD { name, content } => {
                out.push_str(name);
                out.push_str(": ");
                self.render_rule(content, out, indent);
            }

            RawRule::ALIAS {
                content: _,
                value,
                named,
            } => {
                let (open, close) = if *named { ('(', ')') } else { ('"', '"') };
                out.push(open);
                out.push_str(value);
                out.push(close);
            }

            RawRule::TOKEN { content } => {
                self.render_rule(content, out, indent);
            }

            RawRule::IMMEDIATE_TOKEN { content } => {
                self.render_rule(content, out, indent);
                out.push('!');
            }

            RawRule::PREC { content, .. }
            | RawRule::PREC_LEFT { content, .. }
            | RawRule::PREC_RIGHT { content, .. }
            | RawRule::PREC_DYNAMIC { content, .. } => {
                self.render_rule(content, out, indent);
            }

            RawRule::RESERVED { content, .. } => {
                self.render_rule(content, out, indent);
            }
        }
    }

    fn render_block(
        &self,
        children: &[RawRule],
        open: char,
        close: char,
        indent: usize,
        out: &mut String,
    ) {
        out.push(open);
        out.push('\n');

        let child_indent = indent + 1;
        let prefix = "  ".repeat(child_indent);

        for child in children {
            out.push_str(&prefix);
            self.render_rule(child, out, child_indent);
            out.push('\n');
        }

        out.push_str(&"  ".repeat(indent));
        out.push(close);
    }

    fn simplify_optional(&self, children: &[RawRule]) -> Option<RawRule> {
        if children.len() != 2 {
            return None;
        }

        match (&children[0], &children[1]) {
            (RawRule::BLANK, other) | (other, RawRule::BLANK) => Some(other.clone()),
            _ => None,
        }
    }
}
