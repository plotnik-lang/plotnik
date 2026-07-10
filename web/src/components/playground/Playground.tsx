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
import type { Range16 } from "./ast-index";
import { AstView } from "./ast-view";
import { setSpotlight, spotlightExtension } from "./cm-spotlight";
import { CodeEditor } from "./code-editor";
import { OutputPanel, type OutputTab } from "./output-panel";
import { decodePermalink, encodePermalink } from "./permalink";
import { pushQueryFeedback } from "./query-feedback";
import { usePlaygroundSession } from "./use-session";

/* The playground shell: owns the user's inputs (query / source / lang /
   entrypoint choice), feeds them to the session hook, and lays the panes
   out. All engine orchestration lives in use-session.ts; all wire types in
   protocol.ts.

   Two mediator duties live here and nowhere else:
   - editor feedback: pushing each compile's diagnostics/tokens into the
     query editor;
   - cross-pane highlighting: panes report hovers upward, Playground routes
     them to their targets (today: AST hover → source spotlight). New hover
     edges belong here, never wired pane-to-pane — this is the seed of the
     N-way fan-out in docs/wip/playground-design.md §3. */

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

export default function Playground() {
  const initial = useMemo(
    () => decodePermalink(window.location.hash),
    // The hash is read once: later hash edits shouldn't clobber live editing.
    [],
  );

  const [lang, setLang] = useState(initial?.l ?? "javascript");
  const [query, setQuery] = useState(initial?.q ?? DEFAULT_QUERY);
  const [source, setSource] = useState(initial?.s ?? DEFAULT_SOURCE);
  const [entryChoice, setEntryChoice] = useState<string | null>(null);
  const [astOpen, setAstOpen] = useState(false);
  const [outputTab, setOutputTab] = useState<OutputTab>("output");
  const {
    copied: shareCopied,
    show: showShareCopied,
    clear: clearShareCopied,
  } = useCopyFeedback();
  const {
    copied: generatedCopied,
    show: showGeneratedCopied,
    clear: clearGeneratedCopied,
  } = useCopyFeedback();

  const queryViewRef = useRef<EditorView | null>(null);
  const sourceViewRef = useRef<EditorView | null>(null);

  const session = usePlaygroundSession({
    query,
    source,
    lang,
    entry: entryChoice,
    generatedOpen: outputTab === "generated",
  });
  const { compiled } = session;

  useEffect(() => {
    const view = queryViewRef.current;
    if (view && compiled) {
      pushQueryFeedback(
        view,
        compiled.query,
        compiled.info.diagnostics,
        compiled.info.tokens,
      );
    }
  }, [compiled]);

  const share = useCallback(() => {
    const hash = encodePermalink({ q: query, s: source, l: lang });
    const url = new URL(window.location.href);
    url.hash = hash;
    window.history.replaceState(null, "", url);
    void navigator.clipboard
      .writeText(url.toString())
      .then(showShareCopied, clearShareCopied);
  }, [query, source, lang, showShareCopied, clearShareCopied]);

  const copyGenerated = useCallback(() => {
    const code = session.generated?.code;
    if (code === null || code === undefined) return;
    void navigator.clipboard
      .writeText(code)
      .then(showGeneratedCopied, clearGeneratedCopied);
  }, [session.generated, showGeneratedCopied, clearGeneratedCopied]);

  const errorCount =
    compiled?.info.diagnostics.filter(
      (d) => d.severity.toLowerCase() !== "warning",
    ).length ?? 0;
  const warningCount = (compiled?.info.diagnostics.length ?? 0) - errorCount;
  const bytecodeSize = compiled?.info.bytecode_size ?? null;

  const sourceExtensions = useMemo(
    () => [
      javascript({ typescript: lang === "typescript" }),
      spotlightExtension(),
    ],
    [lang],
  );

  /* Hovering an AST node spotlights its source range in the editor. The
     range is in the snapshot's coordinates; the spotlight field maps it
     through any edits made since. */
  const handleAstHover = useCallback((src: Range16 | null) => {
    sourceViewRef.current?.dispatch({
      effects: setSpotlight.of(src === null ? null : [src]),
    });
  }, []);

  return (
    <div className="flex h-dvh flex-col bg-background text-foreground">
      <header className="flex items-center gap-3 border-b bg-sidebar px-4 py-2">
        <a href="/" className="text-sm font-semibold">
          plotnik
        </a>
        <Badge variant="secondary">playground</Badge>
        {!session.ready && (
          <span className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner />
            loading engine…
          </span>
        )}
        <div className="ms-auto flex items-center gap-2">
          {session.compiling && session.ready && <Spinner />}
          {bytecodeSize !== null && (
            <Badge variant="outline">
              {(bytecodeSize / 1024).toFixed(1)} KiB bytecode
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
          {session.entrypoints.length > 1 && (
            <Select
              value={session.entry ?? ""}
              onValueChange={(v) => v && setEntryChoice(v)}
            >
              <SelectTrigger size="sm" className="w-32" aria-label="entrypoint">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {session.entrypoints.map((name) => (
                    <SelectItem key={name} value={name}>
                      {name}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          )}
          <Button variant="outline" size="sm" onClick={share}>
            {shareCopied ? (
              <CheckIcon data-icon="inline-start" />
            ) : (
              <Share2Icon data-icon="inline-start" />
            )}
            {shareCopied ? "Copied" : "Share"}
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
                      value={source}
                      onChange={setSource}
                      extensions={sourceExtensions}
                      onView={(view) => {
                        sourceViewRef.current = view;
                      }}
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
                        <AstView ast={session.ast} onHover={handleAstHover} />
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
                {session.fatal !== null && (
                  <div className="border-t p-3">
                    <Alert variant="destructive">
                      <CircleAlertIcon />
                      <AlertTitle>Compiler gave up</AlertTitle>
                      <AlertDescription>{session.fatal}</AlertDescription>
                    </Alert>
                  </div>
                )}
              </div>
            </ResizablePanel>
            <ResizableHandle withHandle />
            <ResizablePanel defaultSize="45%" minSize="15%">
              <OutputPanel
                info={compiled?.info ?? null}
                generated={session.generated}
                generating={session.generating}
                runResult={session.runResult}
                stale={session.stale}
                tab={outputTab}
                onTabChange={setOutputTab}
                generatedCopied={generatedCopied}
                onGeneratedCopy={copyGenerated}
              />
            </ResizablePanel>
          </ResizablePanelGroup>
        </ResizablePanel>
      </ResizablePanelGroup>
    </div>
  );
}

function useCopyFeedback() {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | null>(null);

  const clear = useCallback(() => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
    setCopied(false);
  }, []);

  const show = useCallback(() => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    setCopied(true);
    timer.current = window.setTimeout(() => {
      timer.current = null;
      setCopied(false);
    }, 1500);
  }, []);

  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );

  return { copied, show, clear };
}

function PaneLabel({ children }: { children: ReactNode }) {
  return (
    <div className="flex items-center gap-2 border-b bg-sidebar px-3 py-1.5 text-xs font-medium tracking-wide text-muted-foreground uppercase">
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
        "flex w-full items-center gap-2 bg-sidebar px-3 py-1.5 text-xs font-medium tracking-wide text-muted-foreground uppercase transition-colors hover:bg-sidebar-accent",
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
