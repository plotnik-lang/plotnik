import * as Comlink from "comlink";
import type { PlotnikApi } from "./protocol";

export type PlotnikClient = Comlink.Remote<PlotnikApi>;

export interface PlotnikClientHandle {
  api: PlotnikClient;

  /** Comlink leaves calls without a handled worker response pending after disposal. */
  dispose(): void;
}

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
