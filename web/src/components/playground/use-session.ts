import { useCallback, useEffect, useRef, useState } from "react";
import { createPlotnikClient, type PlotnikClient } from "./client";
import type {
  AstResult,
  GeneratedCode,
  RunResult,
  SessionInfo,
} from "./protocol";

/* The playground's orchestration layer — and the only module that talks to
   the worker client. It owns the worker lifetime and turns keystroke-level
   inputs into settled engine results; components consume the returned state
   and never call the client themselves.

   Policy encoded here, in one place:
   - one debounced latest-wins pipeline per engine call (compile / run / ast);
   - run re-fires only after a successful compile actually swapped the worker
     session (`compiled` object identity is the generation marker);
   - stale-while-error: a query that stops producing bytecode keeps the last
     good compile info and run output around, flagged `stale`
     (docs/wip/playground-design.md §5). */

export interface SessionInput {
  query: string;
  source: string;
  lang: string;
  /** The user's entrypoint pick; null means "default" (last entrypoint).
      A pick absent from the compiled query falls back, and comes back when
      the entrypoint reappears. */
  entry: string | null;
  /** Generated-code tab is visible and may request its lazy artifact. */
  generatedOpen: boolean;
}

/** A compile result paired with the exact text it describes: every offset in
    `info` is relative to `query`, not to whatever the editor holds now. */
export interface CompileOutcome {
  query: string;
  info: SessionInfo;
}

/** A dump paired with the exact source it describes (same pairing rule). */
export interface AstSnapshot {
  source: string;
  result: AstResult;
}

export interface PlaygroundSession {
  /** Engine worker is up; nothing fires before this. */
  ready: boolean;
  /** A compile is scheduled or in flight. */
  compiling: boolean;
  /** Pathological compiler failure; ordinary errors are `info.diagnostics`. */
  fatal: string | null;
  /** Last good compile (kept through fatal ones). */
  compiled: CompileOutcome | null;
  generated: GeneratedCode | null;
  generating: boolean;
  entrypoints: string[];
  /** Effective entrypoint after fallback; feed it back into the selector. */
  entry: string | null;
  runResult: RunResult | null;
  /** `runResult` came from a query that no longer compiles to a module. */
  stale: boolean;
  ast: AstSnapshot | null;
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
  const [ast, setAst] = useState<AstSnapshot | null>(null);

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
  const hasModule = compiled !== null && compiled.info.bytecode_size !== null;

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
      const result = await clientRef.current?.ast(source, lang, false);
      if (result && stillCurrent()) setAst({ source, result });
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
    ast,
  };
}

/**
 * Debounced latest-wins pipeline: run `task` once `deps` have been quiet for
 * `delayMs`. A newer schedule invalidates every older one; the task must
 * gate its own state writes on `stillCurrent()` after each await. Returns
 * whether a run is scheduled or in flight.
 *
 * `deps` must list everything the task reads that should retrigger it — the
 * task closure itself is read through a ref (so it is never stale), but a
 * missing dep silently skips work.
 */
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

  /* Skip no-op writes: this effect re-runs on every dep change (every
     keystroke), and an unconditional setState from a passive effect during
     a burst of input counts toward React's nested-update limit — enough
     consecutive keystrokes would trip "Maximum update depth exceeded". */
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
      // Also covers unmount: bumping the sequence orphans in-flight results.
      seq.current += 1;
      clearTimeout(timer);
    };
    // Scheduling is driven by the dep list alone; the task reads via ref.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, delayMs, ...deps]);

  return pending;
}
