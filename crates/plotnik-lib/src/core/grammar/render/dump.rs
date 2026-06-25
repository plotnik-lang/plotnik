//! Assembling the full `lang dump` document from a [`TreeGrammar`].
//!
//! Layout: an optional legend, the `extras` line, then three grouped sections —
//! externals, categories, and the node/hidden/token definitions in grammar order.
//! Each definition carries its annotations (`; root`, `; extra`, `; inlined`,
//! `; alias of …`) as a trailing comment on one-liners or a leading `;` line on
//! multi-line bodies.

use super::layout;
use super::{Body, Def, DefKind, TreeGrammar};

/// The baseline column width a group is folded against: a composite that fits
/// within this many columns is rendered inline, otherwise it breaks.
pub const DEFAULT_WIDTH: usize = 80;

/// Options for [`TreeGrammar::dump`].
#[derive(Debug, Clone)]
pub struct DumpOptions {
    /// Emit the self-describing legend header. On by default; `--no-legend` strips it.
    pub legend: bool,
    /// Fold groups inline up to this column width. `0` forces every group to
    /// break (one member per line); a large value keeps everything inline.
    pub width: usize,
}

impl Default for DumpOptions {
    fn default() -> Self {
        Self {
            legend: true,
            width: DEFAULT_WIDTH,
        }
    }
}

impl TreeGrammar {
    /// Render the full tree-shape document.
    pub fn dump(&self, options: &DumpOptions) -> String {
        let width = options.width;
        let mut blocks: Vec<String> = Vec::new();

        if let Some(extras) = self.render_extras(width) {
            blocks.push(extras);
        }
        // Most useful first: categories and node shapes the author copies from,
        // with the opaque external tokens pushed to the end.
        blocks.extend(self.section(width, |kind| matches!(kind, DefKind::Category)));
        blocks.extend(self.section(width, |kind| {
            matches!(
                kind,
                DefKind::Node | DefKind::Hidden | DefKind::Token | DefKind::AliasOf(_)
            )
        }));
        blocks.extend(self.section(width, |kind| matches!(kind, DefKind::External)));

        let mut out = String::new();
        if options.legend {
            out.push_str(&self.legend());
            out.push('\n');
        }
        out.push_str(&blocks.join("\n\n"));
        out.push('\n');
        out
    }

    fn section(&self, width: usize, keep: impl Fn(&DefKind) -> bool) -> Vec<String> {
        self.defs()
            .iter()
            .filter(|def| keep(&def.kind))
            .map(|def| render_def(def, width))
            .collect()
    }

    fn render_extras(&self, width: usize) -> Option<String> {
        if self.extras.is_empty() {
            return None;
        }
        let col = "extras = ".chars().count();
        Some(format!(
            "extras = {}",
            layout::render_list(&self.extras, col, 0, width)
        ))
    }

    fn legend(&self) -> String {
        format!(
            "; {name} — how its trees are shaped, to help write plotnik queries\n\
             ;\n\
             ; A description in query-flavored notation, NOT a runnable query. The\n\
             ; building blocks below are real query syntax you reuse; the rest names\n\
             ; structure the grammar has that a query does not write literally.\n\
             ;\n\
             ; Definitions:\n\
             ;   name = {{...}}     node shape — how a `name` node is built from children\n\
             ;   name# = a | b    category   — `name` is any of a, b, … (tree-sitter supertype)\n\
             ;   name = /regex/   token      — a leaf; the regex is its text, not children\n\
             ;   name = external  external   — a leaf scanned by the grammar's native code\n\
             ;\n\
             ; Query building blocks (reuse these when writing a query):\n\
             ;   (child)   named node     \"text\"  anonymous node (token)\n\
             ;   field: p  field           ? * +  quantifiers\n\
             ;   {{...}}     sequence        [...]   choice (one of)\n\
             ;\n\
             ; Description-only marks (name structure; not written in a query):\n\
             ;   (name#)   here stands a node of category `name` (any of its members)\n\
             ;   _name     a hidden rule — its children show here; not a matchable node\n\
             ;   /regex/   inline token text\n\
             ;\n\
             ; Extras (comments, whitespace) may appear between any two siblings.\n",
            name = self.name()
        )
    }
}

fn render_def(def: &Def, width: usize) -> String {
    let (head, body) = head_and_body(def, width);
    let full = format!("{head}{body}");
    let annotation = annotation(def);

    match annotation {
        None => full,
        Some(note) if full.contains('\n') => format!("; {note}\n{full}"),
        Some(note) => format!("{full}  ; {note}"),
    }
}

/// The `name = ` head and the rendered body for a definition. The body starts at
/// the column just past the head, so folding accounts for the head's width.
fn head_and_body(def: &Def, width: usize) -> (String, String) {
    match &def.body {
        Body::Pattern(shape) => {
            let head = format!("{} = ", def.name);
            let body = layout::render_shape(shape, head.chars().count(), 0, width);
            (head, body)
        }
        Body::Category(members) => {
            let inline = layout::flat_category(members);
            let head = format!("{}# = ", def.name);
            if head.chars().count() + inline.chars().count() <= width {
                (head, inline)
            } else {
                (format!("{}# =", def.name), layout::expand_category(members))
            }
        }
        Body::Token(token) => (format!("{} = ", def.name), token.to_string()),
        // An external token (directly, or aliased under a new name) has no rule body.
        Body::None => (format!("{} = ", def.name), "external".to_string()),
    }
}

fn annotation(def: &Def) -> Option<String> {
    let mut notes = Vec::new();
    if def.root {
        notes.push("root".to_string());
    }
    if def.extra {
        notes.push("extra".to_string());
    }
    // The only hidden rule not already self-marked by a leading underscore is a
    // non-underscore rule placed in the grammar's `inline` list (e.g. devicetree).
    if def.kind == DefKind::Hidden && !def.name.starts_with('_') {
        notes.push("inlined".to_string());
    }
    if let DefKind::AliasOf(target) = &def.kind {
        notes.push(format!("alias of {target}"));
    }

    if notes.is_empty() {
        None
    } else {
        Some(notes.join(", "))
    }
}
