import { useEffect, useRef } from "react";
import {
  EditorState,
  StateEffect,
  StateField,
  type Extension,
} from "@codemirror/state";
import {
  EditorView,
  Decoration,
  type DecorationSet,
  keymap,
  lineNumbers,
  drawSelection,
  highlightSpecialChars,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import {
  bracketMatching,
  foldGutter,
  syntaxHighlighting,
  indentOnInput,
} from "@codemirror/language";
import { classHighlighter } from "@lezer/highlight";
import { cn } from "@/lib/utils";

/* Colors come from the shadcn theme variables so the editors follow the
   site theme (including `.dark`) without a second palette. */
const editorTheme = EditorView.theme({
  "&": {
    height: "100%",
    fontSize: "13px",
    backgroundColor: "transparent",
    color: "var(--color-foreground)",
  },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": {
    fontFamily: "var(--font-code)",
    lineHeight: "1.55",
  },
  ".cm-content": { caretColor: "var(--color-foreground)" },
  ".cm-cursor": { borderLeftColor: "var(--color-foreground)" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground":
    {
      backgroundColor:
        "color-mix(in oklch, var(--color-ring) 24%, transparent)",
    },
  ".cm-activeLine": {
    backgroundColor: "color-mix(in oklch, var(--color-muted) 65%, transparent)",
  },
  ".cm-gutters": {
    backgroundColor: "transparent",
    color: "var(--color-muted-foreground)",
    border: "none",
  },
  ".cm-activeLineGutter": { backgroundColor: "transparent" },
  ".cm-lintRange-error": {
    backgroundImage: "none",
    textDecoration: "underline wavy var(--color-destructive) from-font",
    textUnderlineOffset: "3px",
  },
  ".cm-tooltip": {
    backgroundColor: "var(--color-popover)",
    color: "var(--color-popover-foreground)",
    border: "1px solid var(--color-border)",
    borderRadius: "var(--radius-md)",
  },
});

/* Async decorations (query token highlighting) pushed from outside the
   editor; ranges are mapped through edits until the next push. */
export interface TokenRange {
  from: number;
  to: number;
  cls: string;
}

export const setTokenDecorations = StateEffect.define<TokenRange[]>();

const tokenDecorations = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    let next = value.map(tr.changes);
    for (const effect of tr.effects) {
      if (effect.is(setTokenDecorations)) {
        next = Decoration.set(
          effect.value.map((range) =>
            Decoration.mark({ class: range.cls }).range(range.from, range.to),
          ),
          true,
        );
      }
    }
    return next;
  },
  provide: (field) => EditorView.decorations.from(field),
});

export function baseExtensions(readOnly: boolean): Extension[] {
  const shared: Extension[] = [
    editorTheme,
    lineNumbers(),
    highlightSpecialChars(),
    drawSelection(),
    syntaxHighlighting(classHighlighter),
    EditorView.lineWrapping,
  ];
  if (readOnly) {
    return [
      ...shared,
      foldGutter(),
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
    ];
  }
  return [
    ...shared,
    history(),
    indentOnInput(),
    bracketMatching(),
    tokenDecorations,
    keymap.of([...defaultKeymap, ...historyKeymap]),
  ];
}

export interface CodeEditorProps {
  value: string;
  onChange?: (doc: string) => void;
  extensions?: Extension[];
  readOnly?: boolean;
  className?: string;
  onView?: (view: EditorView) => void;
  "aria-label"?: string;
}

export function CodeEditor({
  value,
  onChange,
  extensions = [],
  readOnly = false,
  className,
  onView,
  "aria-label": ariaLabel,
}: CodeEditorProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const view = new EditorView({
      state: EditorState.create({
        doc: value,
        extensions: [
          ...baseExtensions(readOnly),
          ...extensions,
          EditorView.updateListener.of((update) => {
            if (update.docChanged) {
              onChangeRef.current?.(update.state.doc.toString());
            }
          }),
          EditorView.contentAttributes.of({
            "aria-label": ariaLabel ?? "code editor",
          }),
        ],
      }),
      parent: host,
    });
    viewRef.current = view;
    onView?.(view);

    return () => {
      view.destroy();
      viewRef.current = null;
    };
    // Recreating the view on every prop change would lose selection/history;
    // `value` reconciliation for external updates happens below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const current = view.state.doc.toString();
    if (value !== current) {
      view.dispatch({
        changes: { from: 0, to: current.length, insert: value },
      });
    }
  }, [value]);

  return (
    <div
      ref={hostRef}
      className={cn("h-full min-h-0 overflow-hidden text-left", className)}
    />
  );
}
