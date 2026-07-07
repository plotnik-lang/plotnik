import { useEffect, useRef, useState } from "react";
import {
  Compartment,
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

/* The one CodeMirror wrapper for every editor pane (query, source, output,
   types). The view is created once per mount and reconciled from props:
   `value` by diffing the document, `extensions` through a Compartment — so
   changing either never remounts the editor or loses its history/selection.
   `readOnly` is fixed for the life of the editor. */

/* Colors come from the shadcn theme variables so the editors follow the
   site theme (including `.dark`) without a second palette. */
const editorTheme = EditorView.theme({
  "&": {
    height: "100%",
    fontSize: "13px",
    // The editor's own surface (a step below the app frame). The spotlight
    // lift paints the focused region brighter than this from underneath.
    backgroundColor: "var(--color-editor)",
    color: "var(--color-foreground)",
  },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": {
    fontFamily: "var(--font-code)",
    lineHeight: "1.55",
    // Anchors the spotlight layers (cm-spotlight.ts) and keeps their
    // z-indices scoped to this editor.
    position: "relative",
    isolation: "isolate",
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

function baseExtensions(readOnly: boolean): Extension[] {
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
  /** Extra extensions, swappable at runtime (memoize the array — a new
      identity reconfigures the editor). */
  extensions?: Extension[];
  readOnly?: boolean;
  className?: string;
  /** The escape hatch for imperative CodeMirror work (dispatching effects
      from outside); called with null when the view is destroyed. */
  onView?: (view: EditorView | null) => void;
  "aria-label"?: string;
}

// A stable default: `extensions = []` would make a fresh array per render
// and reconfigure the editor every time.
const NO_EXTENSIONS: Extension[] = [];

export function CodeEditor({
  value,
  onChange,
  extensions = NO_EXTENSIONS,
  readOnly = false,
  className,
  onView,
  "aria-label": ariaLabel,
}: CodeEditorProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const [extras] = useState(() => new Compartment());
  /* Docs the editor emitted whose round-trip through the parent's state may
     still be in flight. Under fast input React can lag several keystrokes
     behind the editor, so the `value` effect must recognize *any* of them as
     an echo — replaying one as a document replacement would clobber what was
     typed since (and cascade: replay → change event → stale state → replay). */
  const pendingEchoes = useRef<string[]>([]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const view = new EditorView({
      state: EditorState.create({
        doc: value,
        extensions: [
          ...baseExtensions(readOnly),
          extras.of(extensions),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) {
              const doc = update.state.doc.toString();
              pendingEchoes.current.push(doc);
              onChangeRef.current?.(doc);
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
      onView?.(null);
      view.destroy();
      viewRef.current = null;
    };
    // Recreating the view on every prop change would lose selection/history;
    // `value` and `extensions` are reconciled by the effects below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const echoes = pendingEchoes.current;
    const echo = echoes.indexOf(value);
    if (echo !== -1) {
      // Our own edit coming back around; props arrive in order, so older
      // pending echoes can never arrive anymore either.
      echoes.splice(0, echo + 1);
      return;
    }
    echoes.length = 0;
    const current = view.state.doc.toString();
    if (value !== current) {
      view.dispatch({
        changes: { from: 0, to: current.length, insert: value },
      });
    }
  }, [value]);

  useEffect(() => {
    viewRef.current?.dispatch({ effects: extras.reconfigure(extensions) });
  }, [extras, extensions]);

  return (
    <div
      ref={hostRef}
      className={cn("h-full min-h-0 overflow-hidden text-left", className)}
    />
  );
}
