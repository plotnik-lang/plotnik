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
  lang: string;
  info: SessionInfo;
}

export type QueryOutput =
  | { status: "pending" }
  | { status: "query-error"; message: string }
  | { status: "complete"; result: RunResult };

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
  output: QueryOutput;
  tree: TreeSnapshot | null;
}

const COMPILE_DEBOUNCE_MS = 250;
const RUN_DEBOUNCE_MS = 150;

export function usePlaygroundSession(input: SessionInput): PlaygroundSession {
  const { query, source, lang, entry, generatedOpen } = input;

  const clientRef = useRef<PlotnikClient | null>(null);
  const [ready, setReady] = useState(false);
  const [failure, setFailure] = useState<{
    query: string;
    lang: string;
    message: string;
  } | null>(null);
  const [compilation, setCompiled] = useState<CompileOutcome | null>(null);
  const [generated, setGenerated] = useState<GeneratedCode | null>(null);
  const [run, setRun] = useState<{
    compiled: CompileOutcome;
    source: string;
    entry: string | null;
    result: RunResult;
  } | null>(null);
  const [tree, setTree] = useState<TreeSnapshot | null>(null);

  const compiled =
    compilation?.query === query && compilation.lang === lang
      ? compilation
      : null;
  const fatal =
    failure?.query === query && failure.lang === lang ? failure.message : null;

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
        setFailure({ query, lang, message: result.fatal });
        setCompiled(null);
        return;
      }
      setFailure(null);
      setGenerated(null);
      setCompiled({ query, lang, info: result.info });
    },
  );

  const entrypoints = compiled?.info.entrypoints ?? [];
  const effectiveEntry =
    entry !== null && entrypoints.includes(entry)
      ? entry
      : (entrypoints[entrypoints.length - 1] ?? null);
  const hasModule =
    compiled !== null &&
    compiled.info.bytecode_size_bytes !== null &&
    fatal === null &&
    !compiling;

  const generating = useDebouncedTask(
    ready && generatedOpen && hasModule && generated === null,
    0,
    [compiled, generatedOpen],
    async (stillCurrent) => {
      const result = await clientRef.current?.generate();
      if (!result || !stillCurrent()) return;
      if ("fatal" in result) {
        setFailure({ query, lang, message: result.fatal });
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
      if (!compiled) return;
      const result = await clientRef.current?.run(
        source,
        effectiveEntry ?? undefined,
      );
      if (result && stillCurrent()) {
        setRun({ compiled, source, entry: effectiveEntry, result });
      }
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

  let output: QueryOutput = { status: "pending" };
  if (fatal !== null) {
    output = { status: "query-error", message: fatal };
  } else if (compiled && compiled.info.bytecode_size_bytes === null) {
    output = {
      status: "query-error",
      message: "Fix the query errors to run it against this source.",
    };
  } else if (
    !compiling &&
    compiled !== null &&
    run?.compiled === compiled &&
    run.source === source &&
    run.entry === effectiveEntry
  ) {
    output = { status: "complete", result: run.result };
  }

  return {
    ready,
    compiling,
    fatal,
    compiled,
    generated: hasModule ? generated : null,
    generating,
    entrypoints,
    entry: effectiveEntry,
    output,
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
