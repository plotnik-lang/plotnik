import { CircleAlertIcon } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Skeleton } from "@/components/ui/skeleton";
import type { AstResult, DumpChunk } from "./protocol";

/* Same palette roles as the query editor: the dump reads like the query that
   would match it. Chunk kinds without an entry render plain. */
const CHUNK_CLASS: Partial<Record<DumpChunk["kind"], string>> = {
  punct: "ptk-punct",
  string: "ptk-string",
  field: "tok-propertyName",
  comment: "ptk-comment",
};

interface AstViewProps {
  result: AstResult | null;
}

export function AstView({ result }: AstViewProps) {
  if (!result) {
    return (
      <div className="flex flex-col gap-2 p-4">
        <Skeleton className="h-4 w-2/5" />
        <Skeleton className="h-4 w-1/2" />
      </div>
    );
  }

  if ("error" in result) {
    return (
      <div className="p-4">
        <Alert variant="destructive">
          <CircleAlertIcon />
          <AlertTitle>Parse failed</AlertTitle>
          <AlertDescription>{result.error}</AlertDescription>
        </Alert>
      </div>
    );
  }

  return (
    <pre className="min-h-full p-3 font-code text-[13px] leading-[1.55]">
      {result.chunks.map((chunk, i) => {
        const cls = CHUNK_CLASS[chunk.kind];
        return cls ? (
          <span key={i} className={cls}>
            {chunk.text}
          </span>
        ) : (
          chunk.text
        );
      })}
    </pre>
  );
}
