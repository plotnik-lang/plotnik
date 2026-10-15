import { byteToUtf16 } from "./byte-offsets";
import type { DumpChunk, DumpNode } from "./protocol";

export interface Range16 {
  from: number;
  to: number;
}

export interface TreeIndexNode {
  src: Range16;

  dump: Range16;

  dumpVisible: Range16;

  parent: number;
}

export interface TreeIndex {
  text: string;

  chunkStarts: number[];

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

function containingNode(index: TreeIndex, offset: number): number | null {
  const nodes = index.nodes;

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

export function lineFirstGlyph(text: string, offset: number): number {
  let i = text.lastIndexOf("\n", Math.max(offset - 1, 0)) + 1;
  while (i < text.length && text[i] === " ") i++;
  return i;
}

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
