import type { Schema } from "../../src/shared/schema";

export function artistDiscographyData(artist: Schema<"CatalogArtistRef">) {
  const cursor = "private-owned-provider-discography:next?edição=二";
  const title = "Sessão do catálogo";
  const credit = "O conjunto da sessão & Voz convidada";
  const first = {
    artist_id: artist.id,
    snapshot: null,
    total_count: null,
    next_cursor: cursor,
    items: [
      {
        id: "opaque:release-group/ao-vivo?edição=一",
        title,
        artist_credit: [
          { artist, credited_name: "O conjunto da sessão", join_phrase: " & " },
          {
            artist: null,
            credited_name: "Voz convidada",
            join_phrase: "",
          },
        ],
        kind: "album",
        secondary_types: ["Live"],
        first_release_date: "2024-02",
        revision: null,
      },
    ],
  } satisfies Schema<"ArtistDiscographyPage">;
  const last = {
    artist_id: artist.id,
    snapshot: null,
    total_count: null,
    next_cursor: null,
    items: [
      {
        id: "opaque:release-group/estúdio?edição=二",
        title,
        artist_credit: null,
        kind: null,
        secondary_types: [],
        first_release_date: null,
        revision: null,
      },
    ],
  } satisfies Schema<"ArtistDiscographyPage">;
  return { cursor, title, credit, first, last };
}
