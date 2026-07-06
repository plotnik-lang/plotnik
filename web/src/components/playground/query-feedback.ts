import { setDiagnostics, type Diagnostic } from "@codemirror/lint";
import type { EditorView } from "@codemirror/view";
import { byteToUtf16 } from "./byte-offsets";
import { setTokenDecorations, type TokenRange } from "./code-editor";
import type { TokenSpan, WireDiagnostic } from "./protocol";

/* Idents and whitespace keep the default ink; everything else gets a class
   styled in global.css next to the lezer `tok-*` palette. */
const TOKEN_CLASS: Partial<Record<TokenSpan["kind"], string>> = {
  comment: "ptk-comment",
  string: "ptk-string",
  regex: "ptk-regex",
  capture: "ptk-capture",
  punct: "ptk-punct",
  error: "ptk-error",
};

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
  tokens: TokenSpan[],
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

  const ranges: TokenRange[] = [];
  for (const token of tokens) {
    const cls = TOKEN_CLASS[token.kind];
    if (!cls || token.start === token.end) continue;
    ranges.push({ from: b2u(token.start), to: b2u(token.end), cls });
  }

  view.dispatch(setDiagnostics(view.state, cmDiagnostics), {
    effects: setTokenDecorations.of(ranges),
  });
}
