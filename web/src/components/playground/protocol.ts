/* Wire types mirrored from the Rust side (plotnik-wasm + plotnik-lib).
   All offsets are BYTE offsets into the original text; convert with
   byte-offsets.ts before handing them to CodeMirror. */

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

/** Inspection spans — plotnik-wasm `spans_json`. */
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

/** `Session::run()` — either a result or a runtime error ("no match" included). */
export type RunResult =
  | {
      value: unknown;
      inspection: unknown;
      stats: RunStats;
      trace?: unknown;
      error?: undefined;
    }
  | { error: string; trace?: unknown };

/** `ast()` output — crates/plotnik-lib/src/core/tree_dump.rs `dump_tree_chunks`.
    A pre-rendered dump of the source tree in Plotnik pattern syntax;
    concatenating `text` yields exactly the CLI's `plotnik ast` output. */
export interface DumpChunk {
  kind: "text" | "punct" | "kind" | "field" | "string" | "comment";
  text: string;
}

export type AstResult = { error: string } | { chunks: DumpChunk[] };

/** Worker API exposed via comlink. */
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
  ast(source: string, lang: string): Promise<AstResult>;
  tokenize(query: string): Promise<TokenSpan[]>;
}
