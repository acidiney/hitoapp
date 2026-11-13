import type { Schema } from "../../../shared/schema";

type Credit = Schema<"CatalogEditionSummary">["artist_credit"];

export type CatalogRow = {
  id: string;
  number: string;
  title: string;
  credit: string | null;
  durationMs: number | null;
};

export function creditText(credit: Credit | undefined): string | null {
  const text = credit
    ?.map((member) => member.credited_name + member.join_phrase)
    .join("");
  return text ? text : null;
}

export function albumRows(
  edition: Schema<"CatalogEditionTrackList">,
  albumCredit: string | null,
): CatalogRow[] {
  const discs = edition.media.length;
  return edition.media.flatMap((medium) =>
    medium.tracks.flatMap((track) =>
      track.kind === "audio"
        ? [
            {
              id: track.id,
              number: `${discs > 1 ? `${medium.position}.` : ""}${track.number ?? track.position}`,
              title: track.title,
              credit: creditText(track.artist_credit) ?? albumCredit,
              durationMs: track.duration_ms ?? null,
            },
          ]
        : [],
    ),
  );
}

export function statsText(stats: Schema<"EditionTrackStats">): string {
  const count = `${stats.audio_tracks} ${stats.audio_tracks === 1 ? "track" : "tracks"}`;
  return stats.total_duration_ms === null
    ? count
    : `${count} · ${longDuration(stats.total_duration_ms)}`;
}

export function longDuration(ms: number): string {
  const minutes = Math.round(ms / 60_000);
  const hours = Math.floor(minutes / 60);
  if (hours === 0) return `${minutes} min`;
  return `${hours} hr ${minutes % 60} min`;
}
