use std::fmt::Write;

use rowan::NodeOrToken;

use crate::ast::{self, SyntaxNode};

use super::Query;
use super::shape_cardinalities::ShapeCardinality;

pub struct QueryPrinter<'q, 'src> {
    query: &'q Query<'src>,
    raw: bool,
    trivia: bool,
    cardinalities: bool,
    spans: bool,
    symbols: bool,
}

impl<'q, 'src> QueryPrinter<'q, 'src> {
    pub fn new(query: &'q Query<'src>) -> Self {
        Self {
            query,
            raw: false,
            trivia: false,
            cardinalities: false,
            spans: false,
            symbols: false,
        }
    }

    pub fn raw(mut self, value: bool) -> Self {
        self.raw = value;
        self
    }

    pub fn with_trivia(mut self, value: bool) -> Self {
        self.trivia = value;
        self
    }

    pub fn with_cardinalities(mut self, value: bool) -> Self {
        self.cardinalities = value;
        self
    }

    pub fn with_spans(mut self, value: bool) -> Self {
        self.spans = value;
        self
    }

    pub fn only_symbols(mut self, value: bool) -> Self {
        self.symbols = value;
        self
    }

    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.format(&mut out).expect("String write never fails");
        out
    }

    pub fn format(&self, w: &mut impl Write) -> std::fmt::Result {
        if self.symbols {
            self.format_symbols(w)
        } else if self.raw {
            self.format_cst(&self.query.syntax(), 0, w)
        } else {
            self.format_root(&self.query.root(), w)
        }
    }

    fn format_symbols(&self, w: &mut impl Write) -> std::fmt::Result {
        use indexmap::IndexSet;
        use std::collections::HashMap;

        let symbols = &self.query.symbols;
        if symbols.is_empty() {
            return Ok(());
        }

        let defined: IndexSet<&str> = symbols.names().collect();

        // Build map from name to body syntax node for cardinality lookup
        let mut body_nodes: HashMap<String, SyntaxNode> = HashMap::new();
        for def in self.query.root().defs() {
            if let (Some(name_tok), Some(body)) = (def.name(), def.body()) {
                body_nodes.insert(name_tok.text().to_string(), body.syntax().clone());
            }
        }

        // Print all definitions in definition order
        for name in symbols.names() {
            let mut visited = IndexSet::new();
            self.format_symbol_tree(name, 0, &defined, &body_nodes, &mut visited, w)?;
        }
        Ok(())
    }

    fn format_symbol_tree(
        &self,
        name: &str,
        indent: usize,
        defined: &indexmap::IndexSet<&str>,
        body_nodes: &std::collections::HashMap<String, SyntaxNode>,
        visited: &mut indexmap::IndexSet<String>,
        w: &mut impl Write,
    ) -> std::fmt::Result {
        let prefix = "  ".repeat(indent);

        if visited.contains(name) {
            writeln!(w, "{}{} (cycle)", prefix, name)?;
            return Ok(());
        }

        let is_broken = !defined.contains(name);
        if is_broken {
            writeln!(w, "{}{}?", prefix, name)?;
            return Ok(());
        }

        let card = body_nodes
            .get(name)
            .map(|n| self.cardinality_mark(n))
            .unwrap_or("");
        writeln!(w, "{}{}{}", prefix, name, card)?;
        visited.insert(name.to_string());

        if let Some(def) = self.query.symbols.get(name) {
            let mut refs: Vec<_> = def.refs.iter().map(|s| s.as_str()).collect();
            refs.sort();
            for r in refs {
                self.format_symbol_tree(r, indent + 1, defined, body_nodes, visited, w)?;
            }
        }

        visited.shift_remove(name);
        Ok(())
    }

    fn format_cst(&self, node: &SyntaxNode, indent: usize, w: &mut impl Write) -> std::fmt::Result {
        let prefix = "  ".repeat(indent);
        let card = self.cardinality_mark(node);
        let span = self.span_str(node.text_range());

        writeln!(w, "{}{:?}{}{}", prefix, node.kind(), card, span)?;

        for child in node.children_with_tokens() {
            match child {
                NodeOrToken::Node(n) => self.format_cst(&n, indent + 1, w)?,
                NodeOrToken::Token(t) => {
                    if self.trivia || !t.kind().is_trivia() {
                        let child_prefix = "  ".repeat(indent + 1);
                        let child_span = self.span_str(t.text_range());
                        writeln!(
                            w,
                            "{}{:?}{} {:?}",
                            child_prefix,
                            t.kind(),
                            child_span,
                            t.text()
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    fn format_root(&self, root: &ast::Root, w: &mut impl Write) -> std::fmt::Result {
        let card = self.cardinality_mark(root.syntax());
        let span = self.span_str(root.syntax().text_range());
        writeln!(w, "Root{}{}", card, span)?;

        for def in root.defs() {
            self.format_def(&def, 1, w)?;
        }
        // Parser wraps all top-level exprs in Def nodes, so this should be empty
        assert!(
            root.exprs().next().is_none(),
            "printer: unexpected bare Expr in Root (parser should wrap in Def)"
        );
        Ok(())
    }

    fn format_def(&self, def: &ast::Def, indent: usize, w: &mut impl Write) -> std::fmt::Result {
        let prefix = "  ".repeat(indent);
        let card = self.cardinality_mark(def.syntax());
        let span = self.span_str(def.syntax().text_range());
        let name = def.name().map(|t| t.text().to_string());

        match name {
            Some(n) => writeln!(w, "{}Def{}{} {}", prefix, card, span, n)?,
            None => writeln!(w, "{}Def{}{}", prefix, card, span)?,
        }

        if let Some(body) = def.body() {
            self.format_expr(&body, indent + 1, w)?;
        }
        Ok(())
    }

    fn format_expr(&self, expr: &ast::Expr, indent: usize, w: &mut impl Write) -> std::fmt::Result {
        let prefix = "  ".repeat(indent);
        let card = self.cardinality_mark(expr.syntax());
        let span = self.span_str(expr.syntax().text_range());

        match expr {
            ast::Expr::Tree(t) => {
                let node_type = t.node_type().map(|tok| tok.text().to_string());
                match node_type {
                    Some(ty) => writeln!(w, "{}Tree{}{} {}", prefix, card, span, ty)?,
                    None => writeln!(w, "{}Tree{}{}", prefix, card, span)?,
                }
                for child in t.children() {
                    self.format_expr(&child, indent + 1, w)?;
                }
            }
            ast::Expr::Ref(r) => {
                let name = r.name().map(|t| t.text().to_string()).unwrap_or_default();
                writeln!(w, "{}Ref{}{} {}", prefix, card, span, name)?;
            }
            ast::Expr::Str(s) => {
                let value = s.value().map(|t| t.text().to_string()).unwrap_or_default();
                writeln!(w, "{}Str{}{} \"{}\"", prefix, card, span, value)?;
            }
            ast::Expr::Alt(a) => {
                writeln!(w, "{}Alt{}{}", prefix, card, span)?;
                for branch in a.branches() {
                    self.format_branch(&branch, indent + 1, w)?;
                }
                for expr in a.exprs() {
                    self.format_expr(&expr, indent + 1, w)?;
                }
            }
            ast::Expr::Seq(s) => {
                writeln!(w, "{}Seq{}{}", prefix, card, span)?;
                for child in s.children() {
                    self.format_expr(&child, indent + 1, w)?;
                }
            }
            ast::Expr::Capture(c) => {
                let name = c.name().map(|t| t.text().to_string()).unwrap_or_default();
                let type_ann = c
                    .type_annotation()
                    .and_then(|t| t.name())
                    .map(|t| t.text().to_string());
                match type_ann {
                    Some(ty) => {
                        writeln!(w, "{}Capture{}{} @{} :: {}", prefix, card, span, name, ty)?
                    }
                    None => writeln!(w, "{}Capture{}{} @{}", prefix, card, span, name)?,
                }
                if let Some(inner) = c.inner() {
                    self.format_expr(&inner, indent + 1, w)?;
                }
            }
            ast::Expr::Quantifier(q) => {
                let op = q
                    .operator()
                    .map(|t| t.text().to_string())
                    .unwrap_or_default();
                writeln!(w, "{}Quantifier{}{} {}", prefix, card, span, op)?;
                if let Some(inner) = q.inner() {
                    self.format_expr(&inner, indent + 1, w)?;
                }
            }
            ast::Expr::Field(f) => {
                let name = f.name().map(|t| t.text().to_string()).unwrap_or_default();
                writeln!(w, "{}Field{}{} {}:", prefix, card, span, name)?;
                if let Some(value) = f.value() {
                    self.format_expr(&value, indent + 1, w)?;
                }
            }
            ast::Expr::NegatedField(f) => {
                let name = f.name().map(|t| t.text().to_string()).unwrap_or_default();
                writeln!(w, "{}NegatedField{}{} !{}", prefix, card, span, name)?;
            }
            ast::Expr::Wildcard(_) => {
                writeln!(w, "{}Wildcard{}{}", prefix, card, span)?;
            }
            ast::Expr::Anchor(_) => {
                writeln!(w, "{}Anchor{}{}", prefix, card, span)?;
            }
        }
        Ok(())
    }

    fn format_branch(
        &self,
        branch: &ast::Branch,
        indent: usize,
        w: &mut impl Write,
    ) -> std::fmt::Result {
        let prefix = "  ".repeat(indent);
        let card = self.cardinality_mark(branch.syntax());
        let span = self.span_str(branch.syntax().text_range());
        let label = branch.label().map(|t| t.text().to_string());

        match label {
            Some(l) => writeln!(w, "{}Branch{}{} {}:", prefix, card, span, l)?,
            None => writeln!(w, "{}Branch{}{}", prefix, card, span)?,
        }

        if let Some(body) = branch.body() {
            self.format_expr(&body, indent + 1, w)?;
        }
        Ok(())
    }

    fn cardinality_mark(&self, node: &SyntaxNode) -> &'static str {
        if !self.cardinalities {
            return "";
        }
        match self.query.shape_cardinality(node) {
            ShapeCardinality::One => "¹",
            ShapeCardinality::Many => "⁺",
        }
    }

    fn span_str(&self, range: rowan::TextRange) -> String {
        if !self.spans {
            return String::new();
        }
        format!(
            " [{}..{}]",
            u32::from(range.start()),
            u32::from(range.end())
        )
    }
}

impl Query<'_> {
    pub fn printer(&self) -> QueryPrinter<'_, '_> {
        QueryPrinter::new(self)
    }
}

#[cfg(test)]
mod tests {
    use crate::Query;
    use indoc::indoc;

    #[test]
    fn printer_with_spans() {
        let q = Query::new("(call)");
        insta::assert_snapshot!(q.printer().with_spans(true).dump(), @r"
        Root [0..6]
          Def [0..6]
            Tree [0..6] call
        ");
    }

    #[test]
    fn printer_with_cardinalities() {
        let q = Query::new("(call)");
        insta::assert_snapshot!(q.printer().with_cardinalities(true).dump(), @r"
        Root¹
          Def¹
            Tree¹ call
        ");
    }

    #[test]
    fn printer_cst_with_trivia() {
        let q = Query::new("(a) (b)");
        insta::assert_snapshot!(q.printer().raw(true).with_trivia(true).dump(), @r#"
        Root
          Def
            Tree
              ParenOpen "("
              Id "a"
              ParenClose ")"
          Whitespace " "
          Def
            Tree
              ParenOpen "("
              Id "b"
              ParenClose ")"
        "#);
    }

    #[test]
    fn printer_alt_branches() {
        let input = indoc! {r#"
            [A: (a) B: (b)]
        "#};
        let q = Query::new(input);
        insta::assert_snapshot!(q.printer().dump(), @r"
        Root
          Def
            Alt
              Branch A:
                Tree a
              Branch B:
                Tree b
        ");
    }

    #[test]
    fn printer_capture_with_type() {
        let q = Query::new("(call)@x :: T");
        insta::assert_snapshot!(q.printer().dump(), @r"
        Root
          Def
            Capture @x :: T
              Tree call
        ");
    }

    #[test]
    fn printer_quantifiers() {
        let q = Query::new("(a)* (b)+ (c)?");
        insta::assert_snapshot!(q.printer().dump(), @r"
        Root
          Def
            Quantifier *
              Tree a
          Def
            Quantifier +
              Tree b
          Def
            Quantifier ?
              Tree c
        ");
    }

    #[test]
    fn printer_field() {
        let q = Query::new("(call name: (id))");
        insta::assert_snapshot!(q.printer().dump(), @r"
        Root
          Def
            Tree call
              Field name:
                Tree id
        ");
    }

    #[test]
    fn printer_negated_field() {
        let q = Query::new("(call !name)");
        insta::assert_snapshot!(q.printer().dump(), @r"
        Root
          Def
            Tree call
              NegatedField !name
        ");
    }

    #[test]
    fn printer_wildcard_and_anchor() {
        let q = Query::new("(call _ . (arg))");
        insta::assert_snapshot!(q.printer().dump(), @r"
        Root
          Def
            Tree call
              Wildcard
              Anchor
              Tree arg
        ");
    }

    #[test]
    fn printer_string_literal() {
        let q = Query::new(r#"(call "foo")"#);
        insta::assert_snapshot!(q.printer().dump(), @r#"
        Root
          Def
            Tree call
              Str "foo"
        "#);
    }

    #[test]
    fn printer_ref() {
        let input = indoc! {r#"
            Expr = (call)
            (func (Expr))
        "#};
        let q = Query::new(input);
        insta::assert_snapshot!(q.printer().dump(), @r"
        Root
          Def Expr
            Tree call
          Def
            Tree func
              Ref Expr
        ");
    }

    #[test]
    fn printer_symbols_with_cardinalities() {
        let input = indoc! {r#"
            A = (a)
            B = {(b) (c)}
            (entry (A) (B))
        "#};
        let q = Query::new(input);
        insta::assert_snapshot!(q.printer().only_symbols(true).with_cardinalities(true).dump(), @r"
        A¹
        B⁺
        ");
    }

    #[test]
    fn printer_symbols_with_refs() {
        let input = indoc! {r#"
            A = (a)
            B = (b (A))
            (entry (B))
        "#};
        let q = Query::new(input);
        insta::assert_snapshot!(q.printer().only_symbols(true).dump(), @r"
        A
        B
          A
        ");
    }

    #[test]
    fn printer_symbols_cycle() {
        let input = indoc! {r#"
            A = [(a) (B)]
            B = [(b) (A)]
            (entry (A))
        "#};
        let q = Query::new(input);
        insta::assert_snapshot!(q.printer().only_symbols(true).dump(), @r"
        A
          B
            A (cycle)
        B
          A
            B (cycle)
        ");
    }

    #[test]
    fn printer_symbols_undefined_ref() {
        let input = "(call (Undefined))";
        let q = Query::new(input);
        insta::assert_snapshot!(q.printer().only_symbols(true).dump(), @"");
    }

    #[test]
    fn printer_symbols_broken_ref() {
        // A defined symbol that references an undefined name
        let input = "A = (foo (Undefined))";
        let q = Query::new(input);
        insta::assert_snapshot!(q.printer().only_symbols(true).dump(), @r"
        A
          Undefined?
        ");
    }

    #[test]
    fn printer_spans_comprehensive() {
        let input = indoc! {r#"
            Foo = (call name: (id) !bar)
            [(a) (b)]
        "#};
        let q = Query::new(input);
        insta::assert_snapshot!(q.printer().with_spans(true).dump(), @r"
        Root [0..39]
          Def [0..28] Foo
            Tree [6..28] call
              Field [12..22] name:
                Tree [18..22] id
              NegatedField [23..27] !bar
          Def [29..38]
            Alt [29..38]
              Branch [30..33]
                Tree [30..33] a
              Branch [34..37]
                Tree [34..37] b
        ");
    }

    #[test]
    fn printer_spans_seq() {
        let q = Query::new("{(a) (b)}");
        insta::assert_snapshot!(q.printer().with_spans(true).dump(), @r"
        Root [0..9]
          Def [0..9]
            Seq [0..9]
              Tree [1..4] a
              Tree [5..8] b
        ");
    }

    #[test]
    fn printer_spans_quantifiers() {
        let q = Query::new("(a)* (b)+");
        insta::assert_snapshot!(q.printer().with_spans(true).dump(), @r"
        Root [0..9]
          Def [0..4]
            Quantifier [0..4] *
              Tree [0..3] a
          Def [5..9]
            Quantifier [5..9] +
              Tree [5..8] b
        ");
    }

    #[test]
    fn printer_spans_alt_branches() {
        let q = Query::new("[A: (a) B: (b)]");
        insta::assert_snapshot!(q.printer().with_spans(true).dump(), @r"
        Root [0..15]
          Def [0..15]
            Alt [0..15]
              Branch [1..7] A:
                Tree [4..7] a
              Branch [8..14] B:
                Tree [11..14] b
        ");
    }
}
