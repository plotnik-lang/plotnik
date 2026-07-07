import * as Comlink from "comlink";
import type { PlotnikApi } from "./protocol";

export type PlotnikClient = Comlink.Remote<PlotnikApi>;

export interface PlotnikClientHandle {
  api: PlotnikClient;
  /** Tears the worker down; in-flight calls never settle after this. */
  dispose(): void;
}

/** Spawn the engine worker and own its lifetime. One handle per mounted
    playground; dispose on unmount or the worker (and its wasm heap) leaks. */
export function createPlotnikClient(): PlotnikClientHandle {
  const worker = new Worker(new URL("./worker.ts", import.meta.url), {
    type: "module",
    name: "plotnik",
  });
  const api = Comlink.wrap<PlotnikApi>(worker);
  return {
    api,
    dispose() {
      api[Comlink.releaseProxy]();
      worker.terminate();
    },
  };
}
