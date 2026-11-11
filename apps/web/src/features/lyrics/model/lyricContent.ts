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

export type TimedLine = Schema<"TimedLyricLine">;

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
  durationMs: number | null;
  timeline: TimedLine[] | null;
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

function anchoredTimeline(events: Schema<"AnchoredLyricLine">[]): TimedLine[] {
  const lines: TimedLine[] = [];
  events.forEach((event, index) => {
    if (event.text.trim() === "") return;
    const next = events
      .slice(index + 1)
      .find((later) => later.start_ms > event.start_ms);
    lines.push({
      start_ms: event.start_ms,
      end_ms: next ? next.start_ms : event.start_ms,
      text: event.text,
    });
  });
  return lines;
}

export const alignmentToleranceMs = 2000;

export function followableLines(
  view: LyricView,
  audioMs: number,
): TimedLine[] | null {
  if (!view.timeline || view.timeline.length < 2) return null;
  const known = audioMs > 0 && view.durationMs !== null;
  const aligned =
    known && Math.abs(view.durationMs! - audioMs) <= alignmentToleranceMs;
  if (view.kind === "anchored")
    return aligned && isSynced(view.timeline) ? view.timeline : null;
  if (known && !aligned) return null;
  return isSynced(view.timeline) ? view.timeline : null;
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

function timedView(lyrics: Schema<"TimedLyrics">): LyricView {
  return {
    kind: "timed",
    synced: isSynced(lyrics.lines),
    label: lyrics.rendition_label ?? null,
    language: lyrics.language ?? null,
    lines: lyrics.lines.map((line) => ({
      text: line.text,
      startMs: line.start_ms,
      endMs: line.end_ms,
    })),
    durationMs: lyrics.duration_ms ?? null,
    timeline: lyrics.lines,
    attribution: null,
    legacyAttribution: lyrics.attribution ?? null,
  };
}

export function legacyLookupOutcome(
  lookup: Schema<"LyricsLookup">,
): SelectedLyricsOutcome {
  if (lookup.status !== "ready") return { kind: lookup.status };
  const lyrics = lookup.document.lyrics;
  return lyrics
    ? { kind: "lyrics", view: timedView(lyrics) }
    : { kind: "not_found" };
}

export function selectedLyricsOutcome(lookup: Lookup): SelectedLyricsOutcome {
  if (lookup.status === "ready_v1") {
    const lyrics = lookup.document.lyrics;
    return lyrics
      ? { kind: "lyrics", view: timedView(lyrics) }
      : { kind: "not_found" };
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
        durationMs: lyrics.duration_ms ?? null,
        timeline:
          lyrics.content.kind === "anchored"
            ? anchoredTimeline(lyrics.content.lines)
            : null,
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
