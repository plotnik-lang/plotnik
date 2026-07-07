/* Wire types, mirrored by hand from the Rust side — every payload here is
   assembled in `crates/plotnik-wasm/src/wire.rs`; change the two together.

   All offsets are BYTE offsets into the exact text that produced them;
   convert with byte-offsets.ts at the point of use, never store UTF-16
   offsets back into these shapes. (Diagnostics also carry line/column, where
   column is a Unicode-scalar count — a third unit; the playground only ever
   reads `offset`.) */

/** `tokenize()` output — crates/plotnik-lib/src/compiler/parse/mod.rs `TokenSpan`. */
export interface TokenSpan {
  kind:
    | "whitespace"
    | "comment"
    | "string"
    | "regex"
    | "capture"
    | "ident"
    | "error"
    | "punct";
  start: number;
  end: number;
}

/** Diagnostics — crates/plotnik-lib/src/compiler/diagnostics/report/json.rs. */
export interface WirePosition {
  line: number;
  column: number;
  offset: number;
}

export interface WireSpan {
  file: string;
  start: WirePosition;
  end: WirePosition;
}

export interface WireDiagnostic {
  code: string;
  severity: "Error" | "Warning" | string;
  message: string;
  span: WireSpan;
  related?: { message: string; span: WireSpan }[];
  fix?: { description: string; replacement: string };
  hints?: string[];
}

/** The static span table: the hub every future cross-view highlight joins
    through (docs/wip/playground-design.md §2). The array index is the SpanId. */
export interface InspectionSpan {
  id: number;
  source: number;
  kind: string;
  start: number;
  end: number;
  type: number | null;
  member: number | null;
}

/** `.d.ts` source map — typegen `DtsRange`. */
export interface DtsRange {
  start: number;
  end: number;
  type_id: number;
  member: number | null;
}

/** `Session::info()`. */
export interface SessionInfo {
  v: number;
  spans: InspectionSpan[];
  tokens: TokenSpan[];
  diagnostics: WireDiagnostic[];
  dts: string;
  dts_map: DtsRange[];
  entrypoints: string[];
  bytecode_size: number | null;
}

export interface RunStats {
  steps_used: number;
  heap_high_water: number;
}

/** `Session::run()`/`trace()` — either a result or a runtime error ("no
    match" included); `trace` is attached to both when recording. */
export type RunResult =
  | {
      value: unknown;
      inspection: unknown;
      stats: RunStats;
      trace?: unknown;
      error?: undefined;
    }
  | { error: string; trace?: unknown };

/** `ast()` output — crates/plotnik-lib/src/core/tree_dump.rs.
    A pre-rendered dump of the source tree in Plotnik pattern syntax;
    concatenating chunk `text` yields exactly the CLI's `plotnik ast` output. */
export interface DumpChunk {
  kind: "text" | "punct" | "kind" | "field" | "string" | "comment";
  text: string;
}

/** Node table entry — `tree_dump.rs` `DumpNode`. Pre-order; dump ranges of
    descendants nest inside their ancestors'. */
export interface DumpNode {
  src_start: number;
  src_end: number;
  dump_start: number;
  dump_end: number;
}

export type AstResult =
  { error: string } | { chunks: DumpChunk[]; nodes: DumpNode[] };

/** Worker API exposed via comlink: the full wasm surface, nothing more.
    Orchestration (debounce, sequencing, caching) belongs in use-session.ts,
    not here. */
export interface PlotnikApi {
  /** Resolves once the wasm module is instantiated. */
  ready(): Promise<void>;
  /**
   * Compile `query` for `lang`, replacing the current session.
   * `fatal` is set only for pathological failures (limits); ordinary
   * query errors come back as structured `info.diagnostics` entries.
   */
  compile(
    query: string,
    lang: string,
  ): Promise<{ info: SessionInfo; fatal?: undefined } | { fatal: string }>;
  /** Run the current session against `source`. */
  run(source: string, entry?: string): Promise<RunResult>;
  /** Run with a bounded execution recording (the debugger's data source). */
  trace(
    source: string,
    entry: string | undefined,
    maxRecords: number,
  ): Promise<RunResult>;
  /** Dump `source`'s tree; `raw` includes anonymous nodes. */
  ast(source: string, lang: string, raw: boolean): Promise<AstResult>;
  tokenize(query: string): Promise<TokenSpan[]>;
}
