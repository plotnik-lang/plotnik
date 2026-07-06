/// <reference lib="webworker" />
import * as Comlink from "comlink";
import init, { Session, ast, tokenize } from "@/lib/plotnik-wasm/plotnik_wasm";
import type { PlotnikApi, SessionInfo, TokenSpan } from "./protocol";

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

  async run(source: string, entry?: string) {
    await wasmReady;
    if (!session) {
      return { error: "no compiled query" };
    }
    return session.run(source, entry ?? null);
  },

  async ast(source: string, lang: string) {
    await wasmReady;
    return ast(source, lang, false);
  },

  async tokenize(query: string) {
    await wasmReady;
    return tokenize(query) as TokenSpan[];
  },
};

Comlink.expose(api);
