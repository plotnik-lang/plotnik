import { json } from "@codemirror/lang-json";
import { javascript } from "@codemirror/lang-javascript";
import { CircleAlertIcon, SearchXIcon } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
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
import type { RunResult, SessionInfo } from "./protocol";

interface OutputPanelProps {
  info: SessionInfo | null;
  runResult: RunResult | null;
  /** Query no longer compiles; the shown output is from the last good run. */
  stale: boolean;
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
          value={JSON.stringify(runResult.value, null, 2)}
          readOnly
          extensions={[json()]}
          aria-label="query output"
        />
      </div>
      <div className="flex items-center gap-2 border-t px-3 py-1.5">
        {stale && <Badge variant="destructive">stale</Badge>}
        <Badge variant="secondary">
          {runResult.stats.steps_used.toLocaleString()} steps
        </Badge>
        <Badge variant="secondary">
          {formatBytes(runResult.stats.heap_high_water)} heap
        </Badge>
      </div>
    </div>
  );
}

function TypesBody({ info }: { info: SessionInfo | null }) {
  if (!info || info.dts.length === 0) {
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
      value={info.dts}
      readOnly
      extensions={[javascript({ typescript: true })]}
      aria-label="inferred TypeScript types"
    />
  );
}

export function OutputPanel({ info, runResult, stale }: OutputPanelProps) {
  return (
    <Tabs defaultValue="output" className="flex h-full min-h-0 flex-col gap-0">
      <div className="flex items-center border-b px-2 py-1.5">
        <TabsList>
          <TabsTrigger value="output">Output</TabsTrigger>
          <TabsTrigger value="types">Types</TabsTrigger>
        </TabsList>
      </div>
      <TabsContent value="output" className="min-h-0 flex-1">
        <OutputBody runResult={runResult} stale={stale} />
      </TabsContent>
      <TabsContent value="types" className="min-h-0 flex-1">
        <TypesBody info={info} />
      </TabsContent>
    </Tabs>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KiB`;
}
