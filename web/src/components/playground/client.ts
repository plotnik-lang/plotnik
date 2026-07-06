import * as Comlink from "comlink";
import type { PlotnikApi } from "./protocol";

export type PlotnikClient = Comlink.Remote<PlotnikApi>;

export function createPlotnikClient(): PlotnikClient {
  const worker = new Worker(new URL("./worker.ts", import.meta.url), {
    type: "module",
    name: "plotnik",
  });
  return Comlink.wrap<PlotnikApi>(worker);
}
