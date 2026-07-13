/* Generic "spotlight" highlight, deliberately pane-agnostic: it knows
   nothing about editors, dumps, or hover — callers translate their own
   geometry into rects/specs. Consumers today: cm-spotlight.ts (CodeMirror
   editors) and tree-dom.ts + tree-view.tsx (the source-tree pane); any future
   pane that joins the cross-view highlight fan-out
   (docs/wip/playground-design.md §3) should adapt itself to this module the
   same way rather than invent another overlay.

   Two layers share one hole path (the union of the focused text's per-line
   rects — possibly from several disjoint ranges):

   - lift: a filled copy of the hole, painted *below* the text, that raises
     the focused region's background (light: to pure white, above the
     off-white canvas). The pop comes from adding light to the region.
   - veil: a full-pane rect masked by the hole, painted *above* the text, that
     dims everything outside. Its color matches the canvas, so it washes out
     the surrounding glyphs without dragging the background below the canvas.

   Either layer can be a no-op per theme (transparent): dark keeps a darkening
   veil and skips the lift; light leans on the lift and a canvas-colored veil.

   The hole is outlined with circular fillets at every corner (convex and
   concave), so the shape never shows a right angle even where lines of
   different width meet. */

export interface RectLike {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export interface SpotlightGeometry {
  /** Corner fillet radius; clamped per-corner to half the adjacent edges. */
  radius: number;
  /** Horizontal breathing room around each line's glyphs. */
  padX: number;
  /** Vertical breathing room above/below each blob. */
  padY: number;
  /** Merge lines whose vertical gap is at most this; larger gaps split the
      region into separate blobs (e.g. across a blank line). */
  bridge: number;
  /** Left/right edges closer than this snap together to avoid micro-jogs. */
  snap: number;
}

export const SPOTLIGHT_GEOMETRY: SpotlightGeometry = {
  /* Very subtle on purpose: enough that no corner is a right angle, not
     enough to read as a pill. */
  radius: 3,
  /* Kept tight: at monospace cell widths, more than ~1px starts swallowing
     the neighboring glyph (worst on punctuation right at the edge). */
  padX: 1,
  padY: 2,
  bridge: 9,
  snap: 2,
};

interface Row {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/* Client rects come per text run (one per styled span); collapse them into
   one rect per visual line before building the outline. */
function mergeRows(rects: RectLike[]): Row[] {
  const sorted = rects
    .filter((r) => r.right - r.left > 0.01)
    .sort((a, b) => a.top - b.top || a.left - b.left);
  const rows: Row[] = [];
  for (const r of sorted) {
    const last = rows[rows.length - 1];
    const midline = (r.top + r.bottom) / 2;
    if (last && midline > last.top && midline < last.bottom) {
      last.left = Math.min(last.left, r.left);
      last.right = Math.max(last.right, r.right);
      last.top = Math.min(last.top, r.top);
      last.bottom = Math.max(last.bottom, r.bottom);
    } else {
      rows.push({ left: r.left, top: r.top, right: r.right, bottom: r.bottom });
    }
  }
  return rows;
}

type Point = [number, number];

/* Outline one blob of vertically contiguous rows, clockwise from the
   top-left corner: across the top, down the right staircase, across the
   bottom, up the left staircase. Jogs between rows are horizontal segments
   at the shared row boundary, so the polygon is rectilinear. */
function blobOutline(rows: Row[]): Point[] {
  const pts: Point[] = [];
  const first = rows[0];
  const last = rows[rows.length - 1];
  pts.push([first.left, first.top], [first.right, first.top]);
  for (let i = 0; i < rows.length - 1; i++) {
    const boundary = rows[i].bottom;
    pts.push([rows[i].right, boundary], [rows[i + 1].right, boundary]);
  }
  pts.push([last.right, last.bottom], [last.left, last.bottom]);
  for (let i = rows.length - 1; i > 0; i--) {
    const boundary = rows[i].top;
    pts.push([rows[i].left, boundary], [rows[i - 1].left, boundary]);
  }
  return pts;
}

function simplify(pts: Point[]): Point[] {
  // Drop zero-length edges (snapped jogs), then collinear middle points.
  const dedup: Point[] = [];
  for (const p of pts) {
    const prev = dedup[dedup.length - 1];
    if (
      prev &&
      Math.abs(prev[0] - p[0]) < 0.05 &&
      Math.abs(prev[1] - p[1]) < 0.05
    )
      continue;
    dedup.push(p);
  }
  while (dedup.length > 1) {
    const head = dedup[0];
    const tail = dedup[dedup.length - 1];
    if (
      Math.abs(head[0] - tail[0]) < 0.05 &&
      Math.abs(head[1] - tail[1]) < 0.05
    )
      dedup.pop();
    else break;
  }
  const out: Point[] = [];
  for (let i = 0; i < dedup.length; i++) {
    const prev = dedup[(i + dedup.length - 1) % dedup.length];
    const next = dedup[(i + 1) % dedup.length];
    const cur = dedup[i];
    const cross =
      (cur[0] - prev[0]) * (next[1] - cur[1]) -
      (cur[1] - prev[1]) * (next[0] - cur[0]);
    if (Math.abs(cross) > 0.05) out.push(cur);
  }
  return out;
}

const fmt = (n: number) => (Math.round(n * 100) / 100).toString();

/* Replace every corner of the rectilinear outline with a circular fillet.
   Cutting distance `d` back along both edges and joining with an arc of
   radius `d` is tangent-continuous for convex and concave corners alike;
   the sweep flag follows the turn direction (cross product, y-down). */
function roundedPath(pts: Point[], radius: number): string {
  if (pts.length < 4) return "";
  const n = pts.length;
  const parts: string[] = [];
  let entry: Point | null = null;
  for (let i = 0; i < n; i++) {
    const prev = pts[(i + n - 1) % n];
    const cur = pts[i];
    const next = pts[(i + 1) % n];
    const inX = cur[0] - prev[0];
    const inY = cur[1] - prev[1];
    const outX = next[0] - cur[0];
    const outY = next[1] - cur[1];
    const inLen = Math.hypot(inX, inY);
    const outLen = Math.hypot(outX, outY);
    const d = Math.min(radius, inLen / 2, outLen / 2);
    const p1: Point = [cur[0] - (inX / inLen) * d, cur[1] - (inY / inLen) * d];
    const p2: Point = [
      cur[0] + (outX / outLen) * d,
      cur[1] + (outY / outLen) * d,
    ];
    const sweep = inX * outY - inY * outX > 0 ? 1 : 0;
    if (entry === null) {
      entry = p1;
      parts.push(`M${fmt(p1[0])} ${fmt(p1[1])}`);
    } else {
      parts.push(`L${fmt(p1[0])} ${fmt(p1[1])}`);
    }
    parts.push(`A${fmt(d)} ${fmt(d)} 0 0 ${sweep} ${fmt(p2[0])} ${fmt(p2[1])}`);
  }
  parts.push("Z");
  return parts.join("");
}

/**
 * Build the spotlight hole path from raw client rects of the focused text
 * (already translated into overlay coordinates). Returns "" when nothing
 * remains to highlight.
 */
export function spotlightPath(
  rects: RectLike[],
  geometry: SpotlightGeometry = SPOTLIGHT_GEOMETRY,
): string {
  const rows = mergeRows(rects);
  if (rows.length === 0) return "";

  // Split into blobs of vertically contiguous rows that also overlap
  // horizontally; disjoint pieces get their own rounded outline.
  const blobs: Row[][] = [];
  let blob: Row[] = [rows[0]];
  for (let i = 1; i < rows.length; i++) {
    const prev = blob[blob.length - 1];
    const cur = rows[i];
    const gap = cur.top - prev.bottom;
    const overlap =
      Math.min(prev.right, cur.right) - Math.max(prev.left, cur.left);
    if (gap <= geometry.bridge && overlap > geometry.padX) {
      blob.push(cur);
    } else {
      blobs.push(blob);
      blob = [cur];
    }
  }
  blobs.push(blob);

  const paths: string[] = [];
  for (const raw of blobs) {
    const rows = raw.map((r) => ({
      left: r.left - geometry.padX,
      right: r.right + geometry.padX,
      top: r.top,
      bottom: r.bottom,
    }));
    // Make rows contiguous (shared boundary at the midpoint of each gap),
    // then pad the blob's outer edges.
    for (let i = 0; i < rows.length - 1; i++) {
      const boundary = (rows[i].bottom + rows[i + 1].top) / 2;
      rows[i].bottom = boundary;
      rows[i + 1].top = boundary;
    }
    rows[0].top -= geometry.padY;
    rows[rows.length - 1].bottom += geometry.padY;
    // Nearly aligned edges snap together so the outline doesn't wiggle.
    for (let i = 0; i < rows.length - 1; i++) {
      if (Math.abs(rows[i].left - rows[i + 1].left) < geometry.snap) {
        const left = Math.min(rows[i].left, rows[i + 1].left);
        rows[i].left = left;
        rows[i + 1].left = left;
      }
      if (Math.abs(rows[i].right - rows[i + 1].right) < geometry.snap) {
        const right = Math.max(rows[i].right, rows[i + 1].right);
        rows[i].right = right;
        rows[i + 1].right = right;
      }
    }
    const path = roundedPath(simplify(blobOutline(rows)), geometry.radius);
    if (path) paths.push(path);
  }
  return paths.join("");
}

export interface SpotlightSpec {
  width: number;
  height: number;
  /** Hole path in overlay coordinates; null hides the layers (with a fade). */
  path: string | null;
}

/** Spec that fades the spotlight out (the last hole shape is kept so the
    exit transition doesn't jump). */
export const HIDDEN_SPOTLIGHT: SpotlightSpec = {
  width: 0,
  height: 0,
  path: null,
};

export interface SpotlightHandle {
  render(spec: SpotlightSpec): void;
  destroy(): void;
}

/**
 * Attach a complete spotlight (lift + veil pair) inside `host`, which must
 * be the positioning ancestor of the highlighted content (both layers are
 * absolutely positioned at its origin; give the host its own stacking
 * context so the layers' z-indices stay scoped).
 */
export function attachSpotlight(host: HTMLElement): SpotlightHandle {
  const lift = createLayer(host, "lift");
  const veil = createLayer(host, "veil");
  return {
    render(spec) {
      lift.render(spec);
      veil.render(spec);
    },
    destroy() {
      lift.destroy();
      veil.destroy();
    },
  };
}

let maskSeq = 0;

const SVG_NS = "http://www.w3.org/2000/svg";

/**
 * Own one spotlight layer, appended to `parent`. `variant` picks which:
 *
 * - `"veil"` (`.spotlight-overlay`, layered above the text): a full-pane rect
 *   masked by the hole, plus a hairline `--spotlight-edge` along its boundary.
 * - `"lift"` (`.spotlight-lift`, layered below the text): a filled copy of the
 *   hole in `--spotlight-lift` that raises the region's background.
 *
 * `render` sizes the layer and swaps the hole. A null path fades the layer
 * out but keeps the last hole so the shape doesn't jump during the exit
 * transition.
 */
function createLayer(
  parent: HTMLElement,
  variant: "veil" | "lift",
): SpotlightHandle {
  const host = parent.appendChild(document.createElement("div"));
  host.classList.add(
    variant === "lift" ? "spotlight-lift" : "spotlight-overlay",
  );
  host.setAttribute("aria-hidden", "true");

  const svg = document.createElementNS(SVG_NS, "svg");
  // Elements that track the pane size, and elements that track the hole path.
  const sizeEls: Element[] = [svg];
  const pathEls: Element[] = [];

  if (variant === "lift") {
    const fill = document.createElementNS(SVG_NS, "path");
    fill.setAttribute("fill", "var(--spotlight-lift)");
    svg.append(fill);
    pathEls.push(fill);
  } else {
    const maskId = `spotlight-mask-${maskSeq++}`;
    const defs = document.createElementNS(SVG_NS, "defs");
    const mask = document.createElementNS(SVG_NS, "mask");
    mask.setAttribute("id", maskId);
    const maskRect = document.createElementNS(SVG_NS, "rect");
    maskRect.setAttribute("fill", "#fff");
    const hole = document.createElementNS(SVG_NS, "path");
    hole.setAttribute("fill", "#000");
    const veil = document.createElementNS(SVG_NS, "rect");
    veil.setAttribute("fill", "var(--spotlight-veil)");
    veil.setAttribute("mask", `url(#${maskId})`);
    /* Hairline along the hole boundary; --spotlight-edge decides per theme
       how visible it is (transparent where the veil alone is enough). */
    const edge = document.createElementNS(SVG_NS, "path");
    edge.setAttribute("fill", "none");
    edge.setAttribute("stroke", "var(--spotlight-edge)");
    edge.setAttribute("stroke-width", "1");
    mask.append(maskRect, hole);
    defs.append(mask);
    svg.append(defs, veil, edge);
    sizeEls.push(maskRect, veil);
    pathEls.push(hole, edge);
  }
  host.append(svg);

  let raf = 0;
  const render = (spec: SpotlightSpec) => {
    cancelAnimationFrame(raf);
    if (spec.path === null || spec.path === "") {
      host.style.opacity = "0";
      return;
    }
    host.style.width = `${spec.width}px`;
    host.style.height = `${spec.height}px`;
    for (const el of sizeEls) {
      el.setAttribute("width", fmt(spec.width));
      el.setAttribute("height", fmt(spec.height));
    }
    for (const el of pathEls) el.setAttribute("d", spec.path);
    if (host.style.opacity !== "1") {
      // Let the hidden state paint once so the entry actually fades.
      raf = requestAnimationFrame(() => {
        host.style.opacity = "1";
      });
    }
  };

  return {
    render,
    destroy() {
      cancelAnimationFrame(raf);
      host.remove();
    },
  };
}

/**
 * Per-line sub-ranges of `[from, to)` with whitespace trimmed from both ends
 * of every line; all-whitespace lines drop out. This is what makes the
 * highlight hug glyphs instead of boxing indentation.
 */
export function lineSegments(
  text: string,
  from: number,
  to: number,
): { from: number; to: number }[] {
  const out: { from: number; to: number }[] = [];
  let lineStart = text.lastIndexOf("\n", Math.max(from - 1, 0)) + 1;
  while (lineStart < to) {
    let lineEnd = text.indexOf("\n", lineStart);
    if (lineEnd === -1) lineEnd = text.length;
    let segFrom = Math.max(from, lineStart);
    let segTo = Math.min(to, lineEnd);
    while (segFrom < segTo && /\s/.test(text[segFrom])) segFrom++;
    while (segTo > segFrom && /\s/.test(text[segTo - 1])) segTo--;
    if (segFrom < segTo) out.push({ from: segFrom, to: segTo });
    lineStart = lineEnd + 1;
  }
  return out;
}
