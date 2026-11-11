import type { Schema } from "../../../shared/schema";
import { releaseCreditLabel } from "../../catalog/model/albumCredit";

export type ShelfCover =
  | { kind: "release"; id: string; artist?: string }
  | { kind: "artist"; id: string; recording: string };

export type ShelfCard = {
  key: string;
  href: string;
  title: string;
  subtitle: string;
  cover: ShelfCover;
};

function albumCard(
  prefix: string,
  album: Pick<
    Schema<"HomeAlbum">,
    "id" | "title" | "artist" | "inferred_artist"
  >,
): ShelfCard {
  const credit = releaseCreditLabel(album);
  return {
    key: `${prefix}-${album.id}`,
    href: `/albums/${encodeURIComponent(album.id)}`,
    title: album.title,
    subtitle: credit ?? "",
    cover: { kind: "release", id: album.id, artist: credit ?? undefined },
  };
}

export function addedShelf(albums: Schema<"AddedAlbum">[]): ShelfCard[] {
  return albums.map((album) => albumCard("added", album));
}

export function artistShelf(
  groups: Schema<"ArtistIdentityGroup">[],
): ShelfCard[] {
  return groups.map((group) => ({
    key: `artist-${group.id}`,
    href: `/artist-groups/${encodeURIComponent(group.id)}`,
    title: group.name.value,
    subtitle: `${group.recording_count} ${group.recording_count === 1 ? "track" : "tracks"}`,
    cover: {
      kind: "artist",
      id: group.id,
      recording: group.name.recording_id,
    },
  }));
}
