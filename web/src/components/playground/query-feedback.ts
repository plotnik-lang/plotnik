import { setDiagnostics, type Diagnostic } from "@codemirror/lint";
import type { EditorView } from "@codemirror/view";
import { byteToUtf16 } from "./byte-offsets";
import { setTokenDecorations, type TokenRange } from "./code-editor";
import type { QueryToken, WireDiagnostic } from "./protocol";

/* Idents and whitespace keep the default ink; everything else gets a class
   styled in global.css next to the lezer `tok-*` palette. Field names are
   the exception: the lexer calls them plain idents, so they are recovered
   below and get the calmer `tok-propertyName` ink, matching the AST pane. */
const TOKEN_CLASS: Partial<Record<QueryToken["kind"], string>> = {
  comment: "ptk-comment",
  string: "ptk-string",
  regex: "ptk-regex",
  capture: "ptk-capture",
  punct: "ptk-punct",
  error: "ptk-error",
};

/* An ident is a field name when a bare `:` follows (variant tags are
   PascalCase names and `::` capture types follow captures, so lowercase + single
   colon is exact), or when it is glued to a leading `-` (negated field). */
function isFieldName(
  tokens: QueryToken[],
  index: number,
  text: (token: QueryToken) => string,
): boolean {
  const ident = tokens[index];
  if (!/^[a-z_]/.test(text(ident))) return false;
  const prev = tokens[index - 1];
  if (
    prev !== undefined &&
    prev.kind === "punct" &&
    prev.end === ident.start &&
    text(prev) === "-"
  ) {
    return true;
  }
  for (let j = index + 1; j < tokens.length; j++) {
    const next = tokens[j];
    if (next.kind === "whitespace" || next.kind === "comment") continue;
    return next.kind === "punct" && text(next) === ":";
  }
  return false;
}

/**
 * Apply a compile round's feedback (squiggles + token colors) to the query
 * editor. `compiledText` is the exact text the worker compiled: if the
 * document moved on since, the byte offsets no longer apply and a newer
 * compile is already in flight, so the push is dropped.
 */
export function pushQueryFeedback(
  view: EditorView,
  compiledText: string,
  diagnostics: WireDiagnostic[],
  tokens: QueryToken[],
): void {
  if (view.state.doc.toString() !== compiledText) return;

  const b2u = byteToUtf16(compiledText);

  const cmDiagnostics: Diagnostic[] = diagnostics.map((diag) => {
    const from = b2u(diag.span.start.offset);
    const to = Math.max(from, b2u(diag.span.end.offset));
    const fix = diag.fix;
    return {
      from,
      to,
      severity: diag.severity.toLowerCase() === "warning" ? "warning" : "error",
      source: diag.code,
      message: diag.hints?.length
        ? `${diag.message}\n${diag.hints.map((hint) => `hint: ${hint}`).join("\n")}`
        : diag.message,
      actions: fix
        ? [
            {
              name: fix.description,
              apply(actionView, actionFrom, actionTo) {
                actionView.dispatch({
                  changes: {
                    from: actionFrom,
                    to: actionTo,
                    insert: fix.replacement,
                  },
                });
              },
            },
          ]
        : undefined,
    };
  });

  const tokenText = (token: QueryToken) =>
    compiledText.slice(b2u(token.start), b2u(token.end));

  const ranges: TokenRange[] = [];
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];
    const cls =
      token.kind === "ident" && isFieldName(tokens, i, tokenText)
        ? "tok-propertyName"
        : TOKEN_CLASS[token.kind];
    if (!cls || token.start === token.end) continue;
    ranges.push({ from: b2u(token.start), to: b2u(token.end), cls });
  }

  view.dispatch(setDiagnostics(view.state, cmDiagnostics), {
    effects: setTokenDecorations.of(ranges),
  });
}
