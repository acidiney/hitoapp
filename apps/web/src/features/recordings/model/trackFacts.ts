import type { Schema } from "../../../shared/schema";
import { relativeTime, time } from "../../../shared/format";

type Format = Schema<"AudioFormat">;

function kilohertz(rate: number): string {
  return Number((rate / 1000).toFixed(3)).toString();
}

function sizeText(bytes: number): string {
  if (bytes >= 1e9) return `${Number((bytes / 1e9).toFixed(2))} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  return `${(bytes / 1e3).toFixed(1)} KB`;
}

export function audioSummary(audio: Format | null): string | null {
  if (!audio) return null;
  const rate = audio.sample_rate_hz;
  const quality =
    audio.bit_depth !== null && rate !== null
      ? `${audio.bit_depth}/${kilohertz(rate)}`
      : rate !== null
        ? `${kilohertz(rate)} kHz`
        : null;
  const text = [audio.format, quality].filter(Boolean).join(" ");
  return text || null;
}

export function audioDetail(audio: Format | null): string | null {
  if (!audio) return null;
  const encoding = [
    audio.format,
    audio.bit_depth !== null ? `${audio.bit_depth}-bit` : null,
  ]
    .filter(Boolean)
    .join(" ");
  const rate =
    audio.sample_rate_hz !== null
      ? `${kilohertz(audio.sample_rate_hz)} kHz`
      : null;
  const quality = [encoding, rate]
    .filter(Boolean)
    .join(audio.bit_depth !== null ? " / " : " ");
  return [quality, sizeText(audio.size_bytes)].filter(Boolean).join(" · ");
}

export function lastPlayedText(
  played: Schema<"LastPlayed"> | null,
  now: number = Date.now(),
): string | null {
  if (!played) return null;
  const when = relativeTime(played.played_at, now);
  return played.position_ms > 0
    ? `${when} · stopped at ${time(played.position_ms / 1000)}`
    : when;
}

export function trackPositionText(
  track: number | null,
  disc: number | null,
  total: number | null,
): string | null {
  if (track === null) return null;
  const position = `Track ${track}${total !== null ? ` of ${total}` : ""}`;
  return disc !== null && disc > 1 ? `Disc ${disc} · ${position}` : position;
}
