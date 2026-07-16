/* Wire types, mirrored by hand from the Rust side — every payload here is
   assembled in `crates/plotnik-wasm/src/wire.rs`; change the two together.

   All offsets are BYTE offsets into the exact text that produced them;
   convert with byte-offsets.ts at the point of use, never store UTF-16
   offsets back into these shapes. (Diagnostics also carry line/column, where
   column is a Unicode-scalar count — a third unit; the playground only ever
   reads `offset`.) */

/** `tokenize()` output — crates/plotnik-lib/src/compiler/parse/mod.rs `QueryToken`. */
export interface QueryToken {
  kind:
    | "whitespace"
    | "comment"
    | "string"
    | "regex"
    | "capture"
    | "ident"
    | "error"
    | "punct";
  span: [number, number];
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
export interface QuerySpan {
  id: number;
  source_id: number;
  kind: string;
  labeling?: "unlabeled" | "labeled";
  span: [number, number];
  binding?: {
    type_id: number;
    member_id?: number;
  };
}

/** TypeScript declaration binding — typegen `TypeScriptBinding`. */
export interface TypeScriptBinding {
  span: [number, number];
  type_id: number;
  member_id: number | null;
}

/** `Session::info()`. */
export interface SessionInfo {
  version: number;
  query_spans: QuerySpan[];
  query_tokens: QueryToken[];
  diagnostics: WireDiagnostic[];
  typescript_declarations: string;
  typescript_bindings: TypeScriptBinding[];
  entry_points: string[];
  bytecode_size_bytes: number | null;
}

/** `Session::generate()` — production generated-code artifact. */
export interface GeneratedCode {
  target: "rust";
  code: string | null;
  grammar: {
    name: string;
    sha256: string;
    source: string;
  };
}

export interface RunStats {
  fuel_used: number;
  peak_live_heap_bytes: number;
}

export interface ResultProvenanceEntry {
  query_span_id: number;
  parent: number | null;
  source_span: [number, number] | null;
  bindings: {
    path: string;
    event_index: number;
  }[];
  range: [number, number];
}

/** `Session::run()`/`trace()` — either a result or a runtime error ("no
    match" included); `execution_trace` is attached to both when recording. */
export type RunResult =
  | {
      result: unknown;
      result_provenance: ResultProvenanceEntry[] | null;
      run_stats: RunStats;
      execution_trace?: unknown;
      error?: undefined;
    }
  | { error: string; execution_trace?: unknown };

/** `tree()` output — crates/plotnik-lib/src/core/tree_dump.rs.
    A pre-rendered dump of the source tree in Plotnik pattern syntax;
    concatenating chunk `text` yields exactly the CLI's `plotnik tree` output. */
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

export type TreeResult =
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
  /** Generate the current session's production Rust matcher. */
  generate(): Promise<GeneratedCode | { fatal: string }>;
  /** Run the current session against `source`. */
  run(source: string, entry?: string): Promise<RunResult>;
  /** Run with a bounded execution recording (the debugger's data source). */
  trace(
    source: string,
    entry: string | undefined,
    maxRecords: number,
  ): Promise<RunResult>;
  /** Dump `source`'s tree, optionally including anonymous nodes. */
  tree(
    source: string,
    lang: string,
    includeAnonymous: boolean,
  ): Promise<TreeResult>;
  tokenize(query: string): Promise<QueryToken[]>;
}
