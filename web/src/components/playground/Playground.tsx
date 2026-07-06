import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import type { EditorView } from "@codemirror/view";
import { javascript } from "@codemirror/lang-javascript";
import {
  CheckIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  CircleAlertIcon,
  Share2Icon,
} from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import { cn } from "@/lib/utils";
import { AstView } from "./ast-view";
import { createPlotnikClient, type PlotnikClient } from "./client";
import { CodeEditor } from "./code-editor";
import { OutputPanel } from "./output-panel";
import { decodePermalink, encodePermalink } from "./permalink";
import type { AstResult, RunResult, SessionInfo } from "./protocol";
import { pushQueryFeedback } from "./query-feedback";

const LANGS = [
  { value: "javascript", label: "JavaScript" },
  { value: "typescript", label: "TypeScript" },
];

const DEFAULT_QUERY = `// Collect every top-level function with its name.
Functions = (program
  (function_declaration
    name: (identifier) @name
  )* @functions
)
`;

const DEFAULT_SOURCE = `function greet(name) {
  return \`hi \${name}\`;
}

function add(a, b) {
  return a + b;
}

const version = 1;
`;

const COMPILE_DEBOUNCE_MS = 250;
const RUN_DEBOUNCE_MS = 150;

export default function Playground() {
  const initial = useMemo(
    () => decodePermalink(window.location.hash),
    // The hash is read once: later hash edits shouldn't clobber live editing.
    [],
  );

  const [lang, setLang] = useState(initial?.l ?? "javascript");
  const [query, setQuery] = useState(initial?.q ?? DEFAULT_QUERY);
  const [source, setSource] = useState(initial?.s ?? DEFAULT_SOURCE);
  const [entry, setEntry] = useState<string | null>(null);
  const [info, setInfo] = useState<SessionInfo | null>(null);
  const [fatal, setFatal] = useState<string | null>(null);
  const [runResult, setRunResult] = useState<RunResult | null>(null);
  const [astResult, setAstResult] = useState<AstResult | null>(null);
  const [astOpen, setAstOpen] = useState(false);
  const [ready, setReady] = useState(false);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);

  const clientRef = useRef<PlotnikClient | null>(null);
  const queryViewRef = useRef<EditorView | null>(null);
  const compileSeq = useRef(0);
  const runSeq = useRef(0);
  const astSeq = useRef(0);
  /* The run target lags the latest compile: run only after the session
     actually swapped (compiledGen bumps on every successful compile call). */
  const [compiledGen, setCompiledGen] = useState(0);

  useEffect(() => {
    const client = createPlotnikClient();
    clientRef.current = client;
    let cancelled = false;
    client.ready().then(() => {
      if (!cancelled) setReady(true);
    });
    return () => {
      cancelled = true;
      clientRef.current = null;
    };
  }, []);

  useEffect(() => {
    if (!ready) return;
    const client = clientRef.current;
    if (!client) return;
    const seq = ++compileSeq.current;
    setBusy(true);
    const timer = setTimeout(async () => {
      const result = await client.compile(query, lang);
      if (seq !== compileSeq.current) return;
      setBusy(false);
      if (result.fatal !== undefined) {
        setFatal(result.fatal);
        return;
      }
      setFatal(null);
      setInfo(result.info);
      setEntry((prev) =>
        prev !== null && result.info.entrypoints.includes(prev) ? prev : null,
      );
      setCompiledGen((gen) => gen + 1);
      const view = queryViewRef.current;
      if (view) {
        pushQueryFeedback(
          view,
          query,
          result.info.diagnostics,
          result.info.tokens,
        );
      }
    }, COMPILE_DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [ready, query, lang]);

  const hasModule = info !== null && info.bytecode_size !== null;

  useEffect(() => {
    if (!ready || !hasModule) return;
    const client = clientRef.current;
    if (!client) return;
    const seq = ++runSeq.current;
    const timer = setTimeout(async () => {
      const result = await client.run(source, entry ?? undefined);
      if (seq !== runSeq.current) return;
      setRunResult(result);
    }, RUN_DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [ready, hasModule, compiledGen, source, entry]);

  useEffect(() => {
    if (!ready) return;
    const client = clientRef.current;
    if (!client) return;
    const seq = ++astSeq.current;
    const timer = setTimeout(async () => {
      const result = await client.ast(source, lang);
      if (seq !== astSeq.current) return;
      setAstResult(result);
    }, RUN_DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [ready, source, lang]);

  const share = useCallback(async () => {
    const hash = encodePermalink({ q: query, s: source, l: lang });
    const url = new URL(window.location.href);
    url.hash = hash;
    window.history.replaceState(null, "", url);
    await navigator.clipboard.writeText(url.toString());
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }, [query, source, lang]);

  const errorCount =
    info?.diagnostics.filter((d) => d.severity.toLowerCase() !== "warning")
      .length ?? 0;
  const warningCount = (info?.diagnostics.length ?? 0) - errorCount;
  const stale = runResult !== null && info !== null && !hasModule;

  const sourceExtensions = useMemo(
    () => [javascript({ typescript: lang === "typescript" })],
    [lang],
  );

  return (
    <div className="flex h-dvh flex-col bg-background text-foreground">
      <header className="flex items-center gap-3 border-b px-4 py-2">
        <a href="/" className="text-sm font-semibold">
          plotnik
        </a>
        <Badge variant="secondary">playground</Badge>
        {!ready && (
          <span className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner />
            loading engine…
          </span>
        )}
        <div className="ms-auto flex items-center gap-2">
          {busy && ready && <Spinner />}
          {info?.bytecode_size != null && (
            <Badge variant="outline">
              {(info.bytecode_size / 1024).toFixed(1)} KiB bytecode
            </Badge>
          )}
          <Select
            value={lang}
            onValueChange={(v) => v && setLang(v)}
            items={LANGS}
          >
            <SelectTrigger size="sm" className="w-36" aria-label="language">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {LANGS.map((l) => (
                  <SelectItem key={l.value} value={l.value}>
                    {l.label}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
          {info !== null && info.entrypoints.length > 1 && (
            <Select
              value={entry ?? info.entrypoints[info.entrypoints.length - 1]}
              onValueChange={(v) => v && setEntry(v)}
            >
              <SelectTrigger size="sm" className="w-32" aria-label="entrypoint">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {info.entrypoints.map((name) => (
                    <SelectItem key={name} value={name}>
                      {name}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          )}
          <Button variant="outline" size="sm" onClick={share}>
            {copied ? (
              <CheckIcon data-icon="inline-start" />
            ) : (
              <Share2Icon data-icon="inline-start" />
            )}
            {copied ? "Copied" : "Share"}
          </Button>
        </div>
      </header>

      <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1">
        <ResizablePanel defaultSize="58%" minSize="25%">
          <div className="flex h-full min-h-0 flex-col">
            <ResizablePanelGroup
              orientation="vertical"
              className="min-h-0 flex-1"
            >
              <ResizablePanel id="source" minSize="20%">
                <div className="flex h-full min-h-0 flex-col">
                  <PaneLabel>source</PaneLabel>
                  <div className="min-h-0 flex-1">
                    <CodeEditor
                      key={lang}
                      value={source}
                      onChange={setSource}
                      extensions={sourceExtensions}
                      aria-label="source code"
                    />
                  </div>
                </div>
              </ResizablePanel>
              {astOpen && (
                <>
                  <ResizableHandle withHandle />
                  <ResizablePanel id="ast" defaultSize="40%" minSize="15%">
                    <div className="flex h-full min-h-0 flex-col">
                      <AstToggle open onToggle={() => setAstOpen(false)} />
                      <div className="min-h-0 flex-1 overflow-auto">
                        <AstView result={astResult} />
                      </div>
                    </div>
                  </ResizablePanel>
                </>
              )}
            </ResizablePanelGroup>
            {!astOpen && (
              <AstToggle
                open={false}
                onToggle={() => setAstOpen(true)}
                className="border-t"
              />
            )}
          </div>
        </ResizablePanel>
        <ResizableHandle withHandle />
        <ResizablePanel defaultSize="42%" minSize="25%">
          <ResizablePanelGroup orientation="vertical">
            <ResizablePanel defaultSize="55%" minSize="15%">
              <div className="flex h-full min-h-0 flex-col">
                <PaneLabel>
                  query
                  {errorCount > 0 && (
                    <Badge variant="destructive">{errorCount}</Badge>
                  )}
                  {warningCount > 0 && (
                    <Badge variant="secondary">{warningCount}</Badge>
                  )}
                </PaneLabel>
                <div className="min-h-0 flex-1">
                  <CodeEditor
                    value={query}
                    onChange={setQuery}
                    onView={(view) => {
                      queryViewRef.current = view;
                    }}
                    aria-label="plotnik query"
                  />
                </div>
                {fatal !== null && (
                  <div className="border-t p-3">
                    <Alert variant="destructive">
                      <CircleAlertIcon />
                      <AlertTitle>Compiler gave up</AlertTitle>
                      <AlertDescription>{fatal}</AlertDescription>
                    </Alert>
                  </div>
                )}
              </div>
            </ResizablePanel>
            <ResizableHandle withHandle />
            <ResizablePanel defaultSize="45%" minSize="15%">
              <OutputPanel info={info} runResult={runResult} stale={stale} />
            </ResizablePanel>
          </ResizablePanelGroup>
        </ResizablePanel>
      </ResizablePanelGroup>
    </div>
  );
}

function PaneLabel({ children }: { children: ReactNode }) {
  return (
    <div className="flex items-center gap-2 border-b px-3 py-1.5 text-xs font-medium tracking-wide text-muted-foreground uppercase">
      {children}
    </div>
  );
}

/* The same bar renders in two homes: as the AST panel's header when open,
   and as the last row of the source column when collapsed. */
function AstToggle({
  open,
  onToggle,
  className,
}: {
  open: boolean;
  onToggle: () => void;
  className?: string;
}) {
  return (
    <button
      type="button"
      onClick={onToggle}
      aria-expanded={open}
      className={cn(
        "flex w-full items-center gap-2 px-3 py-1.5 text-xs font-medium tracking-wide text-muted-foreground uppercase transition-colors hover:bg-muted/50",
        open && "border-b",
        className,
      )}
    >
      {open ? (
        <ChevronDownIcon className="size-3.5" />
      ) : (
        <ChevronRightIcon className="size-3.5" />
      )}
      ast
    </button>
  );
}
