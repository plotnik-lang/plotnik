import {
  RangeSet,
  RangeSetBuilder,
  StateEffect,
  StateField,
  type Extension,
} from "@codemirror/state";
import {
  EditorView,
  GutterMarker,
  ViewPlugin,
  gutterLineClass,
  type ViewUpdate,
} from "@codemirror/view";
import {
  attachSpotlight,
  lineSegments,
  spotlightPath,
  type RectLike,
  type SpotlightHandle,
  type SpotlightSpec,
} from "./spotlight";

/* CodeMirror adapter for the spotlight: dispatch `setSpotlight` with UTF-16
   document ranges (or null) and the editor dims everything outside them.
   This is the editors' sink for the cross-view highlight fan-out — hover
   sources dispatch here (via Playground), and plural ranges are first-class
   because a single query construct can own many source images. The overlay
   lives in scrollDOM, so it scrolls with the content; geometry and viewport
   changes re-measure it. */

export interface SpotlightRange {
  from: number;
  to: number;
}

export const setSpotlight = StateEffect.define<
  readonly SpotlightRange[] | null
>();

const spotlightField = StateField.define<readonly SpotlightRange[] | null>({
  create: () => null,
  update(value, tr) {
    let next = value;
    for (const effect of tr.effects) {
      if (effect.is(setSpotlight)) {
        next = effect.value?.length ? effect.value : null;
      }
    }
    if (next !== null && tr.docChanged) {
      next = next.map((range) => ({
        from: tr.changes.mapPos(range.from),
        to: tr.changes.mapPos(range.to),
      }));
    }
    return next;
  },
});

/** Clamp a spotlight range into the document (edits since the range was
    computed can shrink it away entirely). */
function clampToDoc(range: SpotlightRange, docLength: number): SpotlightRange {
  const from = Math.min(range.from, docLength);
  return { from, to: Math.min(Math.max(range.to, from), docLength) };
}

/* The region's line numbers stay under the veil (cutting them out would
   read as part of the highlight) but render in stronger ink, so even
   dimmed they stand apart from the other numbers. */
const spotlightGutterMarker = new (class extends GutterMarker {
  elementClass = "cm-spotlightGutterLine";
})();

const spotlightGutterLines = gutterLineClass.compute(
  [spotlightField],
  (state) => {
    const regions = state.field(spotlightField);
    if (regions === null) return RangeSet.empty;
    const doc = state.doc;
    const lines = new Set<number>();
    for (const region of regions) {
      const { from, to } = clampToDoc(region, doc.length);
      for (let n = doc.lineAt(from).number; n <= doc.lineAt(to).number; n++) {
        lines.add(n);
      }
    }
    const builder = new RangeSetBuilder<GutterMarker>();
    for (const n of [...lines].sort((a, b) => a - b)) {
      const pos = doc.line(n).from;
      builder.add(pos, pos, spotlightGutterMarker);
    }
    return builder.finish();
  },
);

const spotlightGutterTheme = EditorView.baseTheme({
  ".cm-spotlightGutterLine": {
    color: "var(--color-foreground)",
    fontWeight: "600",
  },
});

const spotlightPlugin = ViewPlugin.fromClass(
  class {
    private spotlight: SpotlightHandle;
    private measureReq: {
      read: (view: EditorView) => SpotlightSpec;
      write: (spec: SpotlightSpec) => void;
    };

    constructor(view: EditorView) {
      // scrollDOM is the positioning ancestor, so the layers scroll with the
      // content (code-editor.tsx gives it position + isolation).
      this.spotlight = attachSpotlight(view.scrollDOM);
      this.measureReq = {
        read: (v) => this.read(v),
        write: (spec) => this.spotlight.render(spec),
      };
      view.requestMeasure(this.measureReq);
    }

    update(update: ViewUpdate) {
      const changed =
        update.state.field(spotlightField) !==
        update.startState.field(spotlightField);
      if (
        changed ||
        update.docChanged ||
        update.viewportChanged ||
        update.geometryChanged
      ) {
        update.view.requestMeasure(this.measureReq);
      }
    }

    read(view: EditorView): SpotlightSpec {
      const scroller = view.scrollDOM;
      const spec: SpotlightSpec = {
        width: scroller.scrollWidth,
        height: scroller.scrollHeight,
        path: null,
      };
      const regions = view.state.field(spotlightField);
      if (regions === null) return spec;

      const hostRect = scroller.getBoundingClientRect();
      const dx = scroller.scrollLeft - hostRect.left;
      const dy = scroller.scrollTop - hostRect.top;
      const doc = view.state.doc;
      const rects: RectLike[] = [];
      for (const region of regions) {
        const { from, to } = clampToDoc(region, doc.length);
        // Positions outside the rendered viewport have no DOM to measure; the
        // veil still covers them, and scrolling re-runs this read.
        const visFrom = Math.max(from, view.viewport.from);
        const visTo = Math.min(to, view.viewport.to);
        if (visFrom >= visTo) continue;

        const text = doc.sliceString(visFrom, visTo);
        for (const seg of lineSegments(text, 0, text.length)) {
          const range = document.createRange();
          const start = view.domAtPos(visFrom + seg.from);
          const end = view.domAtPos(visFrom + seg.to);
          range.setStart(start.node, start.offset);
          range.setEnd(end.node, end.offset);
          for (const rect of range.getClientRects()) {
            rects.push({
              left: rect.left + dx,
              top: rect.top + dy,
              right: rect.right + dx,
              bottom: rect.bottom + dy,
            });
          }
        }
      }
      if (rects.length > 0) spec.path = spotlightPath(rects);
      return spec;
    }

    destroy() {
      this.spotlight.destroy();
    }
  },
);

export function spotlightExtension(): Extension {
  return [
    spotlightField,
    spotlightPlugin,
    spotlightGutterLines,
    spotlightGutterTheme,
  ];
}
