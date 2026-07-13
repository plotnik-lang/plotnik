/// <reference lib="webworker" />
import * as Comlink from "comlink";
import init, { Session, tokenize, tree } from "@/lib/plotnik-wasm/plotnik_wasm";
import type {
  GeneratedCode,
  PlotnikApi,
  SessionInfo,
  QueryToken,
} from "./protocol";

/* The engine side of the playground: owns the wasm instance and the one
   compiled session. Deliberately a dumb pass-through — policy (debounce,
   ordering, stale results) lives on the main thread in use-session.ts. */

const wasmReady = init().then(() => undefined);

/* The session survives failed recompiles: a query that stops compiling
   must not take down the last good run target (stale-while-error). */
let session: Session | null = null;

const api: PlotnikApi = {
  async ready() {
    await wasmReady;
  },

  async compile(query: string, lang: string) {
    await wasmReady;
    try {
      const next = Session.compile(query, lang);
      session?.free();
      session = next;
      return { info: session.info() as SessionInfo };
    } catch (error) {
      return { fatal: String(error) };
    }
  },

  async generate() {
    await wasmReady;
    if (!session) {
      return { fatal: "no compiled query" };
    }
    try {
      return session.generate("rust") as GeneratedCode;
    } catch (error) {
      return { fatal: String(error) };
    }
  },

  async run(source: string, entry?: string) {
    await wasmReady;
    if (!session) {
      return { error: "no compiled query" };
    }
    return session.run(source, entry ?? null);
  },

  async trace(source: string, entry: string | undefined, maxRecords: number) {
    await wasmReady;
    if (!session) {
      return { error: "no compiled query" };
    }
    return session.trace(source, entry ?? null, maxRecords);
  },

  async tree(source: string, lang: string, includeAnonymous: boolean) {
    await wasmReady;
    return tree(source, lang, includeAnonymous);
  },

  async tokenize(query: string) {
    await wasmReady;
    return tokenize(query) as QueryToken[];
  },
};

Comlink.expose(api);
