import { byteToUtf16 } from "./byte-offsets";
import type { DumpChunk, DumpNode } from "./protocol";

/* Index over a source-tree dump for hover interactions: node ranges converted to
   UTF-16 (dump side and source side), plus enough structure to resolve
   "which node is under this dump offset" quickly.

   Pure text/offset logic only — everything that touches the DOM (pointer
   hit-testing, rect measurement) lives in tree-dom.ts. In the playground's
   join-table framing (docs/wip/playground-design.md §2) this is the tree
   pane's table: dump range ↔ source range per node. */

export interface Range16 {
  from: number;
  to: number;
}

export interface TreeIndexNode {
  /** Range in the source text (UTF-16), for the source editor spotlight. */
  src: Range16;
  /** Range in the dump text (UTF-16) used for hover targeting; includes
      the node's `field: ` label so the label is hoverable. */
  dump: Range16;
  /** The visible-highlight range: `dump` minus the `field: ` label. */
  dumpVisible: Range16;
  /** Index of the enclosing node; -1 for the root. */
  parent: number;
}

export interface TreeIndex {
  /** The dump text: all chunk texts concatenated. */
  text: string;
  /** UTF-16 start offset of each chunk in `text` (plus final sentinel). */
  chunkStarts: number[];
  /** Pre-order (sorted by `dump.from`); descendants nest inside ancestors. */
  nodes: TreeIndexNode[];
}

export function buildTreeIndex(
  chunks: DumpChunk[],
  nodes: DumpNode[],
  source: string,
): TreeIndex {
  const chunkStarts: number[] = new Array(chunks.length + 1);
  const startToChunk = new Map<number, number>();
  let text = "";
  for (let i = 0; i < chunks.length; i++) {
    chunkStarts[i] = text.length;
    startToChunk.set(text.length, i);
    text += chunks[i].text;
  }
  chunkStarts[chunks.length] = text.length;

  const dumpB2u = byteToUtf16(text);
  const srcB2u = byteToUtf16(source);
  const indexNodes: TreeIndexNode[] = new Array(nodes.length);
  // Pre-order + nesting: the parent is the nearest open ancestor on a stack.
  const open: number[] = [];
  for (let i = 0; i < nodes.length; i++) {
    const node = nodes[i];
    const dump = { from: dumpB2u(node.dump_start), to: dumpB2u(node.dump_end) };
    while (
      open.length > 0 &&
      indexNodes[open[open.length - 1]].dump.to <= dump.from
    ) {
      open.pop();
    }
    // A node's dump range starts on a chunk boundary: its `field` label
    // chunk when the node sits in a field, its opener otherwise. The
    // visible highlight skips the label and its `: ` punctuation.
    let visibleFrom = dump.from;
    const chunk = startToChunk.get(dump.from);
    if (chunk !== undefined && chunks[chunk].kind === "field") {
      visibleFrom = chunkStarts[Math.min(chunk + 2, chunks.length)];
    }
    indexNodes[i] = {
      src: { from: srcB2u(node.src_start), to: srcB2u(node.src_end) },
      dump,
      dumpVisible: { from: visibleFrom, to: dump.to },
      parent: open.length > 0 ? open[open.length - 1] : -1,
    };
    open.push(i);
  }

  return { text, chunkStarts, nodes: indexNodes };
}

/** Innermost node whose dump range contains `offset`, or null. */
function containingNode(index: TreeIndex, offset: number): number | null {
  const nodes = index.nodes;
  // Binary search: last node starting at or before `offset`. Because ranges
  // nest, walking `parent` links from there finds the innermost container.
  let lo = 0;
  let hi = nodes.length - 1;
  let candidate = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (nodes[mid].dump.from <= offset) {
      candidate = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  while (candidate !== -1 && nodes[candidate].dump.to <= offset) {
    candidate = nodes[candidate].parent;
  }
  return candidate === -1 ? null : candidate;
}

/** Offset of the first non-space glyph on `offset`'s line; can land on the
    newline (blank line) or the end of the text. */
export function lineFirstGlyph(text: string, offset: number): number {
  let i = text.lastIndexOf("\n", Math.max(offset - 1, 0)) + 1;
  while (i < text.length && text[i] === " ") i++;
  return i;
}

/**
 * Map a hovered dump-text offset to a node.
 *
 * The hover area is deliberately larger than the highlight: hovering the
 * indentation left of a line targets the node whose text starts the line,
 * so the whole gutter side of the tree is live. Everywhere else the target
 * is the innermost node containing the offset.
 */
export function resolveDumpHover(
  index: TreeIndex,
  offset: number,
): number | null {
  const text = index.text;
  if (offset < 0 || offset >= text.length) return null;

  const firstGlyph = lineFirstGlyph(text, offset);
  const inIndentation =
    offset <= firstGlyph &&
    firstGlyph < text.length &&
    text[firstGlyph] !== "\n";
  return containingNode(index, inIndentation ? firstGlyph : offset);
}
