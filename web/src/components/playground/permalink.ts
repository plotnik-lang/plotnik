import {
  compressToEncodedURIComponent,
  decompressFromEncodedURIComponent,
} from "lz-string";

export interface PermalinkState {
  q: string;
  s: string;
  l: string;
}

/* `#code/` namespaces the hash so unrelated fragments (heading anchors)
   never get mistaken for a snapshot; bump it if the payload shape changes. */
const PREFIX = "#code/";

export function decodePermalink(hash: string): PermalinkState | null {
  if (!hash.startsWith(PREFIX)) return null;
  try {
    const raw = decompressFromEncodedURIComponent(hash.slice(PREFIX.length));
    if (!raw) return null;
    const data: unknown = JSON.parse(raw);
    if (
      typeof data === "object" &&
      data !== null &&
      typeof (data as PermalinkState).q === "string" &&
      typeof (data as PermalinkState).s === "string" &&
      typeof (data as PermalinkState).l === "string"
    ) {
      return data as PermalinkState;
    }
    return null;
  } catch {
    return null;
  }
}

export function encodePermalink(state: PermalinkState): string {
  return PREFIX + compressToEncodedURIComponent(JSON.stringify(state));
}
