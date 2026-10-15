import { lineFirstGlyph, resolveDumpHover, type TreeIndex } from "./tree-index";
import {
  lineSegments,
  spotlightPath,
  type RectLike,
  type SpotlightSpec,
} from "./spotlight";

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

function caretDumpOffset(
  index: TreeIndex,
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

function positionAt(index: TreeIndex, pre: HTMLPreElement, offset: number) {
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

const LINE_HIT_SLACK = 4;

export function dumpNodeAtPoint(
  index: TreeIndex,
  pre: HTMLPreElement,
  x: number,
  y: number,
): number | null {
  const hit = document.elementFromPoint(x, y)?.closest("[data-ci]");
  if (hit && pre.contains(hit)) {
    const chunk = Number(hit.getAttribute("data-ci"));
    const from = index.chunkStarts[chunk];
    const to = index.chunkStarts[chunk + 1];

    const offset = caretDumpOffset(index, pre, x, y) ?? from;
    return resolveDumpHover(index, Math.min(Math.max(offset, from), to - 1));
  }
  return edgeNode(index, pre, x, y);
}

function edgeNode(
  index: TreeIndex,
  pre: HTMLPreElement,
  x: number,
  y: number,
): number | null {
  const offset = caretDumpOffset(index, pre, x, y);
  if (offset === null) return null;
  const text = index.text;
  const firstGlyph = lineFirstGlyph(text, offset);

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

export function measureDumpSpotlight(
  index: TreeIndex,
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
