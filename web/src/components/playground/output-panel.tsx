import { json } from "@codemirror/lang-json";
import { javascript } from "@codemirror/lang-javascript";
import {
  CheckIcon,
  CircleAlertIcon,
  CopyIcon,
  SearchXIcon,
} from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { CodeEditor } from "./code-editor";
import type { GeneratedCode, RunResult, SessionInfo } from "./protocol";

/* Stable identities: CodeEditor reconfigures whenever the extensions array
   changes, so inline literals would re-init the language on every render. */
const JSON_EXTENSIONS = [json()];
const DTS_EXTENSIONS = [javascript({ typescript: true })];

interface OutputPanelProps {
  info: SessionInfo | null;
  generated: GeneratedCode | null;
  generating: boolean;
  runResult: RunResult | null;
  /** Query no longer compiles; the shown output is from the last good run. */
  stale: boolean;
  tab: OutputTab;
  onTabChange: (tab: OutputTab) => void;
  generatedCopied: boolean;
  onGeneratedCopy: () => void;
}

export type OutputTab = "output" | "types" | "generated";

function GeneratedBody({
  generated,
  generating,
  copied,
  onCopy,
}: {
  generated: GeneratedCode | null;
  generating: boolean;
  copied: boolean;
  onCopy: () => void;
}) {
  if (generating) {
    return (
      <div className="flex flex-col gap-2 p-4">
        <Skeleton className="h-4 w-3/5" />
        <Skeleton className="h-4 w-2/5" />
        <Skeleton className="h-4 w-4/5" />
      </div>
    );
  }
  if (!generated || generated.code === null) {
    return (
      <Empty>
        <EmptyHeader>
          <EmptyTitle>No generated module yet</EmptyTitle>
          <EmptyDescription>
            The compiled Rust matcher appears once the query is valid.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-w-0 items-center gap-2 border-b bg-sidebar px-3 py-1.5">
        <code
          className="min-w-0 flex-1 truncate text-[11px] text-muted-foreground"
          title={`${generated.grammar.name} · ${generated.grammar.source} · SHA-256 ${generated.grammar.sha256}`}
        >
          {generated.grammar.name} · {generated.grammar.source} · sha256:
          {generated.grammar.sha256.slice(0, 12)}
        </code>
        <Button
          type="button"
          variant="outline"
          size="xs"
          onClick={onCopy}
          aria-live="polite"
        >
          {copied ? (
            <CheckIcon data-icon="inline-start" />
          ) : (
            <CopyIcon data-icon="inline-start" />
          )}
          {copied ? "Copied" : "Copy"}
        </Button>
      </div>
      <div className="min-h-0 flex-1">
        <CodeEditor
          value={generated.code}
          readOnly
          aria-label="generated Rust matcher"
        />
      </div>
    </div>
  );
}

function OutputBody({
  runResult,
  stale,
}: {
  runResult: RunResult | null;
  stale: boolean;
}) {
  if (!runResult) {
    return (
      <div className="flex flex-col gap-2 p-4">
        <Skeleton className="h-4 w-2/5" />
        <Skeleton className="h-4 w-3/5" />
        <Skeleton className="h-4 w-1/3" />
      </div>
    );
  }

  if (runResult.error === "no match") {
    return (
      <Empty>
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <SearchXIcon />
          </EmptyMedia>
          <EmptyTitle>No match</EmptyTitle>
          <EmptyDescription>
            The query compiled but matched nothing in this source.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  if (runResult.error !== undefined) {
    return (
      <div className="p-4">
        <Alert variant="destructive">
          <CircleAlertIcon />
          <AlertTitle>Runtime error</AlertTitle>
          <AlertDescription>{runResult.error}</AlertDescription>
        </Alert>
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className={stale ? "min-h-0 flex-1 opacity-50" : "min-h-0 flex-1"}>
        <CodeEditor
          value={JSON.stringify(runResult.result, null, 2)}
          readOnly
          extensions={JSON_EXTENSIONS}
          aria-label="query output"
        />
      </div>
      <div className="flex items-center gap-2 border-t px-3 py-1.5">
        {stale && <Badge variant="destructive">stale</Badge>}
        <Badge variant="secondary">
          {runResult.run_stats.fuel_used.toLocaleString()} fuel
        </Badge>
        <Badge variant="secondary">
          {formatBytes(runResult.run_stats.peak_live_heap_bytes)} peak heap
        </Badge>
      </div>
    </div>
  );
}

function TypesBody({ info }: { info: SessionInfo | null }) {
  if (!info || info.typescript_declarations.length === 0) {
    return (
      <Empty>
        <EmptyHeader>
          <EmptyTitle>No types yet</EmptyTitle>
          <EmptyDescription>
            Types appear once the query compiles.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }
  return (
    <CodeEditor
      value={info.typescript_declarations}
      readOnly
      extensions={DTS_EXTENSIONS}
      aria-label="inferred TypeScript types"
    />
  );
}

export function OutputPanel({
  info,
  generated,
  generating,
  runResult,
  stale,
  tab,
  onTabChange,
  generatedCopied,
  onGeneratedCopy,
}: OutputPanelProps) {
  return (
    <Tabs
      value={tab}
      onValueChange={(value) => onTabChange(value as OutputTab)}
      className="flex h-full min-h-0 flex-col gap-0"
    >
      <div className="flex items-center border-b bg-sidebar px-2 py-1.5">
        <TabsList>
          <TabsTrigger value="output">Output</TabsTrigger>
          <TabsTrigger value="types">Types</TabsTrigger>
          <TabsTrigger value="generated">Generated</TabsTrigger>
        </TabsList>
      </div>
      <TabsContent value="output" className="min-h-0 flex-1">
        <OutputBody runResult={runResult} stale={stale} />
      </TabsContent>
      <TabsContent value="types" className="min-h-0 flex-1">
        <TypesBody info={info} />
      </TabsContent>
      <TabsContent value="generated" className="min-h-0 flex-1">
        <GeneratedBody
          generated={generated}
          generating={generating}
          copied={generatedCopied}
          onCopy={onGeneratedCopy}
        />
      </TabsContent>
    </Tabs>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KiB`;
}
