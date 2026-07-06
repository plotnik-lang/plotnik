import {
  createHighlighter,
  type DecorationItem,
  type ThemeRegistration,
} from "shiki";
import { plotnikGrammar } from "./plotnik-grammar";

/* Placeholder syntax roles. The scope→role mapping in makeTheme below is the
   part worth keeping: which TextMate scopes share a color, across the plotnik
   grammar plus TypeScript and JSON. The color values here are deliberately
   provisional neutrals — the old brand-tuned palette was removed with the rest
   of the design. When the new design lands, give capture/type/value/op distinct
   hues (and re-check contrast against whatever the pane background becomes). */
interface SyntaxRoles {
  /** Default code ink (node kinds, plain identifiers). */
  fg: string;
  /** Softer structural ink (keywords, field names). */
  fgSoft: string;
  /** Punctuation and brackets. */
  punct: string;
  /** Comments. */
  comment: string;
  capture: string;
  type: string;
  value: string;
  op: string;
}

const rolesDark: SyntaxRoles = {
  fg: "#e6e6e6",
  fgSoft: "#b9b9b9",
  punct: "#8f8f8f",
  comment: "#8f8f8f",
  capture: "#e6e6e6",
  type: "#e6e6e6",
  value: "#e6e6e6",
  op: "#e6e6e6",
};

const rolesLight: SyntaxRoles = {
  fg: "#2a2a2a",
  fgSoft: "#4a4a4a",
  punct: "#6a6a6a",
  comment: "#6a6a6a",
  capture: "#2a2a2a",
  type: "#2a2a2a",
  value: "#2a2a2a",
  op: "#2a2a2a",
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
      { scope: ["constant.numeric"], settings: { foreground: p.value } },
      {
        scope: ["constant.language"],
        settings: { foreground: p.fgSoft },
      },
      {
        scope: [
          "keyword",
          "storage.type",
          "storage.modifier",
          "keyword.operator",
        ],
        settings: { foreground: p.fgSoft },
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
        settings: { foreground: p.fgSoft },
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
        settings: { foreground: p.fg },
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
