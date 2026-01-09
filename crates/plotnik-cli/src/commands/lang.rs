use std::collections::HashSet;
use std::process::exit;

use plotnik_core::grammar::{Grammar, Rule};

/// List all supported languages with aliases.
pub fn run_list() {
    let infos = plotnik_langs::all_info();
    for info in infos {
        let aliases: Vec<_> = info.aliases.iter().skip(1).copied().collect();
        if aliases.is_empty() {
            println!("{}", info.name);
        } else {
            println!("{} ({})", info.name, aliases.join(", "));
        }
    }
}

/// Dump grammar for a language.
pub fn run_dump(lang_name: &str) {
    let Some(lang) = plotnik_langs::from_name(lang_name) else {
        eprintln!("error: unknown language '{lang_name}'");
        eprintln!();
        eprintln!("Run 'plotnik lang list' to see available languages.");
        exit(1);
    };

    let grammar = lang.grammar();
    let renderer = GrammarRenderer::new(grammar);
    print!("{}", renderer.render());
}

pub struct GrammarRenderer<'a> {
    grammar: &'a Grammar,
    hidden_rules: HashSet<&'a str>,
}

impl<'a> GrammarRenderer<'a> {
    pub fn new(grammar: &'a Grammar) -> Self {
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

    fn render_rule_list(&self, label: &str, rules: &[Rule], out: &mut String) {
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
            if let Some((_, rule)) = self.grammar.rules.iter().find(|(n, _)| n == supertype) {
                out.push_str(supertype);
                out.push_str(" :: supertype = ");
                self.render_rule(rule, out, 0);
                out.push_str("\n\n");
            }
        }
    }

    fn render_rules(&self, out: &mut String) {
        let supertypes_set: HashSet<_> = self.grammar.supertypes.iter().collect();

        for (name, rule) in &self.grammar.rules {
            if supertypes_set.contains(name) {
                continue;
            }

            out.push_str(name);
            out.push_str(" = ");
            self.render_rule(rule, out, 0);
            out.push_str("\n\n");
        }
    }

    fn render_rule(&self, rule: &Rule, out: &mut String, indent: usize) {
        match rule {
            Rule::Blank => out.push_str("()"),

            Rule::String(s) => {
                out.push('"');
                for c in s.chars() {
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

            Rule::Pattern { value, flags } => {
                out.push('/');
                out.push_str(value);
                out.push('/');
                if let Some(f) = flags {
                    out.push_str(f);
                }
            }

            Rule::Symbol(name) => {
                out.push('(');
                out.push_str(name);
                if self.hidden_rules.contains(name.as_str()) {
                    out.push_str(" ...)");
                } else {
                    out.push(')');
                }
            }

            Rule::Seq(children) => {
                self.render_block(children, '{', '}', indent, out);
            }

            Rule::Choice(children) => {
                if let Some(simplified) = self.simplify_optional(children) {
                    self.render_rule(&simplified, out, indent);
                    out.push('?');
                } else {
                    self.render_block(children, '[', ']', indent, out);
                }
            }

            Rule::Repeat(inner) => {
                self.render_rule(inner, out, indent);
                out.push('*');
            }

            Rule::Repeat1(inner) => {
                self.render_rule(inner, out, indent);
                out.push('+');
            }

            Rule::Field { name, content } => {
                out.push_str(name);
                out.push_str(": ");
                self.render_rule(content, out, indent);
            }

            Rule::Alias {
                content: _,
                value,
                named,
            } => {
                let (open, close) = if *named { ('(', ')') } else { ('"', '"') };
                out.push(open);
                out.push_str(value);
                out.push(close);
            }

            Rule::Token(inner) => {
                self.render_rule(inner, out, indent);
            }

            Rule::ImmediateToken(inner) => {
                self.render_rule(inner, out, indent);
                out.push('!');
            }

            Rule::Prec { content, .. }
            | Rule::PrecLeft { content, .. }
            | Rule::PrecRight { content, .. }
            | Rule::PrecDynamic { content, .. } => {
                self.render_rule(content, out, indent);
            }

            Rule::Reserved { content, .. } => {
                self.render_rule(content, out, indent);
            }
        }
    }

    fn render_block(
        &self,
        children: &[Rule],
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

    fn simplify_optional(&self, children: &[Rule]) -> Option<Rule> {
        if children.len() != 2 {
            return None;
        }

        match (&children[0], &children[1]) {
            (Rule::Blank, other) | (other, Rule::Blank) => Some(other.clone()),
            _ => None,
        }
    }
}
