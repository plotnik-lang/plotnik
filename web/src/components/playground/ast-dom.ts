import { lineFirstGlyph, resolveDumpHover, type AstIndex } from "./ast-index";
import {
  lineSegments,
  spotlightPath,
  type RectLike,
  type SpotlightSpec,
} from "./spotlight";

/* DOM-side geometry for the AST dump pane: pointer → dump offset → node
   (hit-testing) and node → client rects → spotlight spec (measurement).

   Rendering contract with ast-view.tsx: the dump renders inside a wrapper
   (the spotlight's positioning ancestor) as a <pre> whose children are one
   <span data-ci={i}> per chunk, in chunk order, so `pre.children[i]` is
   chunk i and `index.chunkStarts` maps between chunk-local and global dump
   offsets. Anything that walks that DOM belongs here, not in the view. */

/* Resolve a pointer position to a text offset. Chrome/Safari and Firefox
   expose different halves of the same API. */
function caretAtPoint(
  x: number,
  y: number,
): { node: Node; offset: number } | null {
  const doc = document as Document & {
    caretPositionFromPoint?: (
      x: number,
      y: number,
    ) => { offsetNode: Node; offset: number } | null;
  };
  if (typeof doc.caretPositionFromPoint === "function") {
    const pos = doc.caretPositionFromPoint(x, y);
    return pos ? { node: pos.offsetNode, offset: pos.offset } : null;
  }
  const range = document.caretRangeFromPoint?.(x, y);
  return range
    ? { node: range.startContainer, offset: range.startOffset }
    : null;
}

/** The caret nearest the pointer as a global dump offset, or null when it
    doesn't land inside one of `pre`'s chunk spans. */
function caretDumpOffset(
  index: AstIndex,
  pre: HTMLPreElement,
  x: number,
  y: number,
): number | null {
  const caret = caretAtPoint(x, y);
  if (!caret) return null;
  const el =
    caret.node instanceof Element ? caret.node : caret.node.parentElement;
  const span = el?.closest("[data-ci]");
  if (!span || !pre.contains(span)) return null;
  return index.chunkStarts[Number(span.getAttribute("data-ci"))] + caret.offset;
}

/* Dump offset → (text node, local offset); binary search over chunk starts,
   then straight into that chunk's span. */
function positionAt(index: AstIndex, pre: HTMLPreElement, offset: number) {
  const starts = index.chunkStarts;
  let lo = 0;
  let hi = starts.length - 2;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (starts[mid] <= offset) lo = mid;
    else hi = mid - 1;
  }
  const el = pre.children[lo];
  return { node: el.firstChild ?? el, offset: offset - starts[lo], chunk: lo };
}

/* Vertical slack when matching a pointer to a line's glyph box; mirrors the
   spans' hit padding so edge hovering is continuous between lines too. */
const LINE_HIT_SLACK = 4;

/**
 * The node under the pointer, or null over dead space.
 *
 * Two hit paths. Direct: the pointer is on a glyph (chunk spans are the only
 * hover targets; empty space hits the <pre> itself). Edge: everything left
 * of a line — down to the pane's own edge, so a maximized window lets the
 * cursor rest at the screen edge — acts as that line's hover area, while
 * space right of a line and below the tree stays dead.
 */
export function dumpNodeAtPoint(
  index: AstIndex,
  pre: HTMLPreElement,
  x: number,
  y: number,
): number | null {
  const hit = document.elementFromPoint(x, y)?.closest("[data-ci]");
  if (hit && pre.contains(hit)) {
    const chunk = Number(hit.getAttribute("data-ci"));
    const from = index.chunkStarts[chunk];
    const to = index.chunkStarts[chunk + 1];
    // The caret API returns the nearest boundary, which can fall just past
    // the hovered chunk; clamp back so edge pixels can't target a neighbor.
    const offset = caretDumpOffset(index, pre, x, y) ?? from;
    return resolveDumpHover(index, Math.min(Math.max(offset, from), to - 1));
  }
  return edgeNode(index, pre, x, y);
}

function edgeNode(
  index: AstIndex,
  pre: HTMLPreElement,
  x: number,
  y: number,
): number | null {
  const offset = caretDumpOffset(index, pre, x, y);
  if (offset === null) return null;
  const text = index.text;
  const firstGlyph = lineFirstGlyph(text, offset);
  // The caret snapped past the line's first glyph: the pointer sits right
  // of the text (or below the tree), not in the left margin.
  if (offset > firstGlyph) return null;
  if (firstGlyph >= text.length || text[firstGlyph] === "\n") return null;
  const glyph = positionAt(index, pre, firstGlyph);
  const range = document.createRange();
  range.setStart(glyph.node, glyph.offset);
  range.setEnd(glyph.node, glyph.offset + 1);
  const rect = range.getBoundingClientRect();
  if (x > rect.left) return null;
  if (y < rect.top - LINE_HIT_SLACK || y > rect.bottom + LINE_HIT_SLACK) {
    return null;
  }
  return resolveDumpHover(index, firstGlyph);
}

/**
 * Spotlight spec for `node`'s visible dump range, in `wrap`'s coordinate
 * space (`wrap` is the spotlight's positioning ancestor around the <pre>).
 */
export function measureDumpSpotlight(
  index: AstIndex,
  pre: HTMLPreElement,
  wrap: HTMLElement,
  node: number,
): SpotlightSpec {
  const wrapRect = wrap.getBoundingClientRect();
  const { from, to } = index.nodes[node].dumpVisible;
  const rects: RectLike[] = [];
  const range = document.createRange();
  for (const seg of lineSegments(index.text, from, to)) {
    /* Measure chunk by chunk so the range never fully contains a span:
       a fully-contained element reports its border box, and the spans
       carry hit-area padding that must not leak into the visible shape.
       Staying inside one text node yields pure glyph boxes. */
    let pos = seg.from;
    while (pos < seg.to) {
      const start = positionAt(index, pre, pos);
      const end = Math.min(seg.to, index.chunkStarts[start.chunk + 1]);
      range.setStart(start.node, start.offset);
      range.setEnd(start.node, start.offset + (end - pos));
      for (const rect of range.getClientRects()) {
        rects.push({
          left: rect.left - wrapRect.left,
          top: rect.top - wrapRect.top,
          right: rect.right - wrapRect.left,
          bottom: rect.bottom - wrapRect.top,
        });
      }
      pos = end;
    }
  }
  return {
    width: wrap.offsetWidth,
    height: wrap.offsetHeight,
    path: spotlightPath(rects),
  };
}
