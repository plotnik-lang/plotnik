import { useCallback, useLayoutEffect, useMemo, useRef } from "react";
import { CircleAlertIcon } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";
import { dumpNodeAtPoint, measureDumpSpotlight } from "./ast-dom";
import { buildAstIndex, type Range16 } from "./ast-index";
import type { DumpChunk, DumpNode } from "./protocol";
import {
  attachSpotlight,
  HIDDEN_SPOTLIGHT,
  type SpotlightHandle,
} from "./spotlight";
import type { AstSnapshot } from "./use-session";

/* The AST pane. AstView picks the state (skeleton / parse error / dump);
   DumpView renders the dump and owns the hover machinery, so the spotlight
   and index live exactly as long as there is a dump to point into.

   Hover is one-way by design: the pane reports the hovered node's *source*
   range upward via onHover and touches nothing else — Playground mediates
   all cross-pane highlighting (see the fan-out plan in
   docs/wip/playground-design.md §3). */

/* Same palette roles as the query editor: the dump reads like the query that
   would match it. Chunk kinds without an entry render plain. */
const CHUNK_CLASS: Partial<Record<DumpChunk["kind"], string>> = {
  punct: "ptk-punct",
  string: "ptk-string",
  field: "tok-propertyName",
  comment: "ptk-comment",
};

interface AstViewProps {
  ast: AstSnapshot | null;
  /** Source range of the hovered node (UTF-16, in the snapshot's source
      coordinates), or null; fires only on change. */
  onHover?: (src: Range16 | null) => void;
}

export function AstView({ ast, onHover }: AstViewProps) {
  if (!ast) {
    return (
      <div className="flex flex-col gap-2 p-4">
        <Skeleton className="h-4 w-2/5" />
        <Skeleton className="h-4 w-1/2" />
      </div>
    );
  }

  if ("error" in ast.result) {
    return (
      <div className="p-4">
        <Alert variant="destructive">
          <CircleAlertIcon />
          <AlertTitle>Parse failed</AlertTitle>
          <AlertDescription>{ast.result.error}</AlertDescription>
        </Alert>
      </div>
    );
  }

  return (
    <DumpView
      chunks={ast.result.chunks}
      nodes={ast.result.nodes}
      source={ast.source}
      onHover={onHover}
    />
  );
}

interface DumpViewProps {
  chunks: DumpChunk[];
  nodes: DumpNode[];
  source: string;
  onHover?: (src: Range16 | null) => void;
}

function DumpView({ chunks, nodes, source, onHover }: DumpViewProps) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const preRef = useRef<HTMLPreElement>(null);
  const spotRef = useRef<SpotlightHandle | null>(null);
  const hoverRef = useRef<number | null>(null);

  const index = useMemo(
    () => buildAstIndex(chunks, nodes, source),
    [chunks, nodes, source],
  );
  const indexRef = useRef(index);
  indexRef.current = index;
  const onHoverRef = useRef(onHover);
  onHoverRef.current = onHover;

  const setHover = useCallback((node: number | null) => {
    if (node === hoverRef.current) return;
    hoverRef.current = node;
    const index = indexRef.current;
    const pre = preRef.current;
    const wrap = wrapRef.current;
    if (spotRef.current && pre && wrap) {
      spotRef.current.render(
        node === null
          ? HIDDEN_SPOTLIGHT
          : measureDumpSpotlight(index, pre, wrap, node),
      );
    }
    onHoverRef.current?.(node === null ? null : index.nodes[node].src);
  }, []);

  useLayoutEffect(() => {
    const wrap = wrapRef.current;
    if (!wrap) return;
    const spotlight = attachSpotlight(wrap);
    spotRef.current = spotlight;
    return () => {
      spotlight.destroy();
      spotRef.current = null;
      // The dump is going away; don't leave the source editor lit.
      onHoverRef.current?.(null);
    };
  }, []);

  // A new dump invalidates node indices; drop the hover rather than let it
  // point at the wrong node until the pointer moves again.
  useLayoutEffect(() => {
    setHover(null);
  }, [index, setHover]);

  return (
    /* `isolate` keeps the spotlight layers' z-index scoped to this wrapper;
       `bg-editor` is the surface the lift rises above. */
    <div
      ref={wrapRef}
      className="relative isolate w-max min-w-full min-h-full bg-editor"
    >
      <pre
        ref={preRef}
        className="min-h-full select-none p-3 font-code text-[13px] leading-[1.55]"
        onPointerMove={(event) => {
          const pre = preRef.current;
          if (pre) {
            setHover(dumpNodeAtPoint(index, pre, event.clientX, event.clientY));
          }
        }}
        onPointerLeave={() => setHover(null)}
      >
        {/* One span per chunk, in order — the ast-dom.ts contract. Vertical
            padding on the inline spans doesn't move any text (it never grows
            the line box) but it does extend their hit area into the
            half-leading gap between lines, so hover stays continuous when
            the pointer crosses from one line to the next. */}
        {chunks.map((chunk, i) => (
          <span
            key={i}
            data-ci={i}
            className={cn("py-1", CHUNK_CLASS[chunk.kind])}
          >
            {chunk.text}
          </span>
        ))}
      </pre>
    </div>
  );
}
