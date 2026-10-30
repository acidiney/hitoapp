import type { Schema } from "../../src/shared/schema";

export function artistEditionsData(
  group: Schema<"CatalogReleaseGroup">,
  artist: Schema<"CatalogArtistRef">,
) {
  const title = "Sessão nº 1";
  const cursor = "private-owned-edition-cursor:next?versão=二";
  const snapshot = "private-owned-edition-snapshot:one";
  const credit = [
    { artist, credited_name: "O conjunto da sessão", join_phrase: "" },
  ];
  const standard = {
    id: "opaque:edition/padrão?versão=一",
    title,
    artist_credit: credit,
    disambiguation: "Standard edition",
    release_date: "2024-02",
    country: "PT",
    revision: null,
  } satisfies Schema<"CatalogEditionSummary">;
  const bonus = {
    id: "opaque:edition/bónus?versão=二",
    title,
    artist_credit: null,
    disambiguation: "Bonus edition",
    release_date: null,
    country: null,
    revision: "private-owned-edition-revision:bonus",
  } satisfies Schema<"CatalogEditionSummary">;
  const first = {
    release_group_id: group.id,
    snapshot,
    items: [standard],
    total_count: null,
    next_cursor: cursor,
  } satisfies Schema<"ReleaseGroupEditionsPage">;
  const last = {
    ...first,
    items: [bonus],
    next_cursor: null,
  } satisfies Schema<"ReleaseGroupEditionsPage">;
  const tracks = {
    id: bonus.id,
    release_group_id: group.id,
    title,
    artist_credit: null,
    revision: bonus.revision,
    media: [
      {
        position: 1,
        title: "The session",
        format: "CD",
        tracks: [
          {
            id: "opaque:occurrence/bonus/1/0",
            position: 0,
            number: "0",
            kind: "pregap",
            title: "Introdução de ensaio",
            artist_credit: null,
            duration_ms: null,
            recording_id: null,
          },
          {
            id: "opaque:occurrence/bonus/1/1",
            position: 1,
            number: "1",
            kind: "audio",
            title: "Tema repetido",
            artist_credit: credit,
            duration_ms: 125_000,
            recording_id: "opaque:recording/repeated?take=一",
          },
        ],
      },
      {
        position: 2,
        title: null,
        format: null,
        tracks: [
          {
            id: "opaque:occurrence/bonus/2/1",
            position: 1,
            number: "A1",
            kind: "audio",
            title: "Tema repetido",
            artist_credit: null,
            duration_ms: 126_000,
            recording_id: "opaque:recording/repeated?take=一",
          },
          {
            id: "opaque:occurrence/bonus/2/2",
            position: 2,
            number: null,
            kind: "data",
            title: "Material de sessão",
            artist_credit: null,
            duration_ms: null,
            recording_id: null,
          },
        ],
      },
    ],
  } satisfies Schema<"CatalogEditionTrackList">;
  const privateValues = [
    title,
    cursor,
    snapshot,
    standard.id,
    bonus.id,
    bonus.revision,
    ...credit.map((member) => member.credited_name),
    ...tracks.media.flatMap((medium) => [
      ...(medium.title ? [medium.title] : []),
      ...medium.tracks.flatMap((track) => [
        track.id,
        track.title,
        ...(track.recording_id ? [track.recording_id] : []),
      ]),
    ]),
  ];
  return {
    group,
    title,
    cursor,
    snapshot,
    standard,
    bonus,
    first,
    last,
    tracks,
    privateValues,
  };
}

export type ArtistEditionsData = ReturnType<typeof artistEditionsData>;
