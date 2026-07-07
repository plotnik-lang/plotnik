import {
  createHighlighter,
  type DecorationItem,
  type ThemeRegistration,
} from "shiki";
import { plotnikGrammar } from "./plotnik-grammar";

/* Syntax roles, colored from the jrnv Zed theme so the static panes match
   the CodeMirror `tok-*`/`ptk-*` classes in global.css: one role, one color,
   in both schemes. Keywords, operators, and field/property names stay ink
   (the theme's signature), types are blue, strings green, constants/captures
   purple, structure muted. Light string green is #3e8123 (theme's #448c27
   misses AA on white). */
interface SyntaxRoles {
  /** Default code ink (node kinds, plain identifiers, keywords, operators). */
  fg: string;
  /** Field names: a calmer ink, matching `tok-propertyName`. */
  field: string;
  /** Punctuation and brackets. */
  punct: string;
  /** Comments. */
  comment: string;
  capture: string;
  type: string;
  /** String and regex literals. */
  value: string;
  /** Numeric and boolean literals. */
  num: string;
  op: string;
}

const rolesDark: SyntaxRoles = {
  fg: "#d5dde4",
  field: "#95a3b0",
  punct: "#828f9c",
  comment: "#828f9c",
  capture: "#b49bd8",
  type: "#7aa7e6",
  value: "#82b56d",
  num: "#b49bd8",
  op: "#d5dde4",
};

const rolesLight: SyntaxRoles = {
  fg: "#000000",
  field: "#454c54",
  punct: "#66717d",
  comment: "#66717d",
  capture: "#7a3e9d",
  type: "#325cc0",
  value: "#3e8123",
  num: "#7a3e9d",
  op: "#000000",
};

/* One semantic role per color, the same in every pane, so the eye can follow
   a capture from the query into the type and into the data. */
function makeTheme(
  name: string,
  type: "dark" | "light",
  p: SyntaxRoles,
): ThemeRegistration {
  return {
    name,
    type,
    colors: {
      "editor.background": "#00000000",
      "editor.foreground": p.fg,
    },
    settings: [
      { settings: { foreground: p.fg } },
      {
        scope: ["comment", "punctuation.definition.comment"],
        settings: { foreground: p.comment },
      },
      {
        scope: ["string", "punctuation.definition.string", "string.regexp"],
        settings: { foreground: p.value },
      },
      { scope: ["constant.numeric"], settings: { foreground: p.num } },
      {
        scope: ["constant.language"],
        settings: { foreground: p.num },
      },
      {
        scope: [
          "keyword",
          "storage.type",
          "storage.modifier",
          "keyword.operator",
        ],
        settings: { foreground: p.fg },
      },
      {
        scope: ["punctuation", "meta.brace", "punctuation.bracket"],
        settings: { foreground: p.punct },
      },

      /* plotnik */
      {
        scope: ["variable.capture.plotnik"],
        settings: { foreground: p.capture },
      },
      {
        scope: [
          "entity.name.type.definition.plotnik",
          "entity.name.type.reference.plotnik",
          "entity.name.type.variant.plotnik",
          "entity.name.type.annotation.plotnik",
        ],
        settings: { foreground: p.type },
      },
      {
        scope: ["entity.name.tag.plotnik"],
        settings: { foreground: p.fg },
      },
      {
        scope: ["variable.other.member.plotnik"],
        settings: { foreground: p.field },
      },
      {
        scope: [
          "keyword.operator.quantifier.plotnik",
          "keyword.operator.anchor.plotnik",
          "keyword.operator.comparison.plotnik",
        ],
        settings: { foreground: p.op },
      },

      /* TypeScript */
      {
        scope: [
          "entity.name.type",
          "support.type.primitive",
          "support.type.builtin",
        ],
        settings: { foreground: p.type },
      },
      {
        scope: ["variable.object.property", "meta.object-literal.key"],
        settings: { foreground: p.capture },
      },
      {
        scope: ["entity.name.function"],
        settings: { foreground: p.type },
      },

      /* JSON: keys mirror the query's captures; span offsets stay quiet */
      {
        scope: ["support.type.property-name.json"],
        settings: { foreground: p.capture },
      },
      {
        scope: [
          "meta.structure.dictionary.value.json string.quoted.double.json",
        ],
        settings: { foreground: p.value },
      },
      {
        scope: ["constant.numeric.json"],
        settings: { foreground: p.punct },
      },
    ],
  };
}

const highlighterPromise = createHighlighter({
  themes: [
    makeTheme("plotnik-dark", "dark", rolesDark),
    makeTheme("plotnik-light", "light", rolesLight),
  ],
  langs: [plotnikGrammar, "typescript", "json"],
});

/** A decorated range: an exact substring (nth occurrence) or a byte span. */
export type MarkSpec = { class?: string } & (
  { find: string; occurrence?: number | "all" } | { start: number; end: number }
);

function markDecorations(code: string, marks: MarkSpec[]): DecorationItem[] {
  const out: DecorationItem[] = [];
  for (const mark of marks) {
    const cls = mark.class ?? "cap-mark";
    if ("start" in mark) {
      out.push({
        start: mark.start,
        end: mark.end,
        properties: { class: cls },
      });
      continue;
    }
    const wanted = mark.occurrence ?? 1;
    let from = 0;
    let n = 0;
    for (;;) {
      const at = code.indexOf(mark.find, from);
      if (at === -1) break;
      n += 1;
      if (wanted === "all" || n === wanted) {
        out.push({
          start: at,
          end: at + mark.find.length,
          properties: { class: cls },
        });
        if (wanted !== "all") break;
      }
      from = at + mark.find.length;
    }
    if (n === 0 || (typeof wanted === "number" && n < wanted)) {
      throw new Error(`mark not found: ${JSON.stringify(mark.find)}`);
    }
  }
  return out;
}

export async function highlight(
  code: string,
  lang: string,
  marks: MarkSpec[] = [],
): Promise<string> {
  const highlighter = await highlighterPromise;
  return highlighter.codeToHtml(code.trimEnd(), {
    lang,
    themes: { light: "plotnik-light", dark: "plotnik-dark" },
    defaultColor: false,
    decorations: markDecorations(code.trimEnd(), marks),
  });
}
