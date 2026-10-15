import { setDiagnostics, type Diagnostic } from "@codemirror/lint";
import type { EditorView } from "@codemirror/view";
import { byteToUtf16 } from "./byte-offsets";
import { setTokenDecorations, type TokenRange } from "./code-editor";
import type { QueryToken, WireDiagnostic } from "./protocol";

const TOKEN_CLASS: Partial<Record<QueryToken["kind"], string>> = {
  comment: "ptk-comment",
  string: "ptk-string",
  regex: "ptk-regex",
  capture: "ptk-capture",
  punct: "ptk-punct",
  error: "ptk-error",
};

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
    prev.span[1] === ident.span[0] &&
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
    compiledText.slice(b2u(token.span[0]), b2u(token.span[1]));

  const ranges: TokenRange[] = [];
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];
    const cls =
      token.kind === "ident" && isFieldName(tokens, i, tokenText)
        ? "tok-propertyName"
        : TOKEN_CLASS[token.kind];
    if (!cls || token.span[0] === token.span[1]) continue;
    ranges.push({ from: b2u(token.span[0]), to: b2u(token.span[1]), cls });
  }

  view.dispatch(setDiagnostics(view.state, cmDiagnostics), {
    effects: setTokenDecorations.of(ranges),
  });
}
