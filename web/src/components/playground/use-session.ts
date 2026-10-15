import { useCallback, useEffect, useRef, useState } from "react";
import { createPlotnikClient, type PlotnikClient } from "./client";
import type {
  GeneratedCode,
  RunResult,
  SessionInfo,
  TreeResult,
} from "./protocol";

export interface SessionInput {
  query: string;
  source: string;
  lang: string;

  entry: string | null;

  generatedOpen: boolean;
}

export interface CompileOutcome {
  query: string;
  info: SessionInfo;
}

export interface TreeSnapshot {
  source: string;
  result: TreeResult;
}

export interface PlaygroundSession {
  ready: boolean;

  compiling: boolean;

  fatal: string | null;

  compiled: CompileOutcome | null;
  generated: GeneratedCode | null;
  generating: boolean;
  entrypoints: string[];

  entry: string | null;
  runResult: RunResult | null;

  stale: boolean;
  tree: TreeSnapshot | null;
}

const COMPILE_DEBOUNCE_MS = 250;
const RUN_DEBOUNCE_MS = 150;

export function usePlaygroundSession(input: SessionInput): PlaygroundSession {
  const { query, source, lang, entry, generatedOpen } = input;

  const clientRef = useRef<PlotnikClient | null>(null);
  const [ready, setReady] = useState(false);
  const [fatal, setFatal] = useState<string | null>(null);
  const [compiled, setCompiled] = useState<CompileOutcome | null>(null);
  const [generated, setGenerated] = useState<GeneratedCode | null>(null);
  const [runResult, setRunResult] = useState<RunResult | null>(null);
  const [tree, setTree] = useState<TreeSnapshot | null>(null);

  useEffect(() => {
    const handle = createPlotnikClient();
    clientRef.current = handle.api;
    let cancelled = false;
    void handle.api.ready().then(() => {
      if (!cancelled) setReady(true);
    });
    return () => {
      cancelled = true;
      clientRef.current = null;
      handle.dispose();
    };
  }, []);

  const compiling = useDebouncedTask(
    ready,
    COMPILE_DEBOUNCE_MS,
    [query, lang],
    async (stillCurrent) => {
      const result = await clientRef.current?.compile(query, lang);
      if (!result || !stillCurrent()) return;
      if (result.fatal !== undefined) {
        setFatal(result.fatal);
        return;
      }
      setFatal(null);
      setGenerated(null);
      setCompiled({ query, info: result.info });
    },
  );

  const entrypoints = compiled?.info.entrypoints ?? [];
  const effectiveEntry =
    entry !== null && entrypoints.includes(entry)
      ? entry
      : (entrypoints[entrypoints.length - 1] ?? null);
  const hasModule =
    compiled !== null && compiled.info.bytecode_size_bytes !== null;

  const generating = useDebouncedTask(
    ready && generatedOpen && hasModule && generated === null,
    0,
    [compiled, generatedOpen],
    async (stillCurrent) => {
      const result = await clientRef.current?.generate();
      if (!result || !stillCurrent()) return;
      if ("fatal" in result) {
        setFatal(result.fatal);
        return;
      }
      setGenerated(result);
    },
  );

  useDebouncedTask(
    ready && hasModule,
    RUN_DEBOUNCE_MS,
    [compiled, source, effectiveEntry],
    async (stillCurrent) => {
      const result = await clientRef.current?.run(
        source,
        effectiveEntry ?? undefined,
      );
      if (result && stillCurrent()) setRunResult(result);
    },
  );

  useDebouncedTask(
    ready,
    RUN_DEBOUNCE_MS,
    [source, lang],
    async (stillCurrent) => {
      const result = await clientRef.current?.tree(source, lang, false);
      if (result && stillCurrent()) setTree({ source, result });
    },
  );

  return {
    ready,
    compiling,
    fatal,
    compiled,
    generated,
    generating,
    entrypoints,
    entry: effectiveEntry,
    runResult,
    stale: runResult !== null && compiled !== null && !hasModule,
    tree,
  };
}

function useDebouncedTask(
  enabled: boolean,
  delayMs: number,
  deps: readonly unknown[],
  task: (stillCurrent: () => boolean) => Promise<void>,
): boolean {
  const [pending, setPending] = useState(false);
  const pendingRef = useRef(false);
  const taskRef = useRef(task);
  taskRef.current = task;
  const seq = useRef(0);

  const publishPending = useCallback((next: boolean) => {
    if (pendingRef.current === next) return;
    pendingRef.current = next;
    setPending(next);
  }, []);

  useEffect(() => {
    if (!enabled) {
      publishPending(false);
      return;
    }
    const id = ++seq.current;
    publishPending(true);
    const timer = setTimeout(() => {
      void taskRef
        .current(() => id === seq.current)
        .finally(() => {
          if (id === seq.current) publishPending(false);
        });
    }, delayMs);
    return () => {
      seq.current += 1;
      clearTimeout(timer);
    };

    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, delayMs, ...deps]);

  return pending;
}
