import type { Schema } from "../../../shared/schema";
import { isSynced } from "./lineTiming";
import {
  sameProviderRef,
  sameSelector,
  type LyricIdentity,
} from "./lyricIdentity";

type Lookup = Schema<"SelectedLyricsLookup">;
export type ReadyLookup = Extract<Lookup, { status: "ready_v1" | "ready_v2" }>;

export type DisplayLine = {
  text: string;
  startMs: number | null;
  endMs: number | null;
};

export type LyricAttributionView = {
  providerLabel: string;
  contentUrl: string;
};

export type LyricView = {
  kind: "timed" | "plain" | "anchored";
  synced: boolean;
  label: string | null;
  language: string | null;
  lines: DisplayLine[];
  attribution: LyricAttributionView | null;
  legacyAttribution: Schema<"RecordingAttribution"> | null;
};

export type SelectedLyricsOutcome =
  | { kind: "lyrics"; view: LyricView }
  | { kind: "not_found" }
  | { kind: "unsupported" }
  | { kind: "unavailable" }
  | { kind: "cancelled" };

function withStanzaBreaks(lines: DisplayLine[]): DisplayLine[] {
  const kept: DisplayLine[] = [];
  for (const line of lines) {
    const blank = line.text.trim() === "";
    if (blank && (kept.length === 0 || kept[kept.length - 1].text === ""))
      continue;
    kept.push(blank ? { ...line, text: "" } : line);
  }
  while (kept.length > 0 && kept[kept.length - 1].text === "") kept.pop();
  return kept;
}

function attributionOf(
  attribution: Schema<"LyricAttribution"> | null | undefined,
): LyricAttributionView | null {
  return attribution
    ? {
        providerLabel: attribution.provider_label,
        contentUrl: attribution.content_url,
      }
    : null;
}

export function selectedLyricsOutcome(lookup: Lookup): SelectedLyricsOutcome {
  if (lookup.status === "ready_v1") {
    const lyrics = lookup.document.lyrics;
    if (!lyrics) return { kind: "not_found" };
    return {
      kind: "lyrics",
      view: {
        kind: "timed",
        synced: isSynced(lyrics.lines),
        label: lyrics.rendition_label ?? null,
        language: lyrics.language ?? null,
        lines: lyrics.lines.map((line) => ({
          text: line.text,
          startMs: line.start_ms,
          endMs: line.end_ms,
        })),
        attribution: null,
        legacyAttribution: lyrics.attribution ?? null,
      },
    };
  }
  if (lookup.status === "ready_v2") {
    const lyrics = lookup.document.lyrics;
    if (!lyrics) return { kind: "not_found" };
    const lines: DisplayLine[] =
      lyrics.content.kind === "plain"
        ? lyrics.content.lines.map((text) => ({
            text,
            startMs: null,
            endMs: null,
          }))
        : withStanzaBreaks(
            lyrics.content.lines.map((line) => ({
              text: line.text,
              startMs: null,
              endMs: null,
            })),
          );
    return {
      kind: "lyrics",
      view: {
        kind: lyrics.content.kind,
        synced: false,
        label: lyrics.rendition_label ?? null,
        language: lyrics.language ?? null,
        lines,
        attribution: attributionOf(lyrics.attribution),
        legacyAttribution: null,
      },
    };
  }
  return { kind: lookup.status };
}

export function previewLines(view: LyricView | null): string[] {
  return view ? view.lines.map((line) => line.text) : [];
}

export function readMatchesIdentity(
  result: ReadyLookup,
  recording: string,
  identity: LyricIdentity,
): boolean {
  return (
    result.recording_id === recording &&
    sameSelector(result.selector, identity.selector) &&
    sameProviderRef(result.provider_recording, identity.provider_recording) &&
    sameProviderRef(result.document.recording, identity.provider_recording)
  );
}
