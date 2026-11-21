import type { Schema } from "../../../shared/schema";
import { releaseCreditLabel } from "../../catalog/model/albumCredit";

export type ShelfCover =
  | { kind: "release"; id: string; artist?: string }
  | { kind: "artist"; id: string; recording?: string; photo: boolean };

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

export function playedShelf(albums: Schema<"HomeAlbum">[]): ShelfCard[] {
  return albums.map((album) => albumCard("played", album));
}

export function addedShelf(albums: Schema<"AddedAlbum">[]): ShelfCard[] {
  return albums.map((album) => albumCard("added", album));
}

function count(value: number, one: string, many: string) {
  return `${value} ${value === 1 ? one : many}`;
}

function artistCount(artist: Schema<"Artist">) {
  if (artist.recording_count > 0)
    return count(artist.recording_count, "track", "tracks");
  if (artist.release_count > 0)
    return count(artist.release_count, "album", "albums");
  return count(
    artist.featured_recording_count ?? 0,
    "appearance",
    "appearances",
  );
}

export function artistShelf(artists: Schema<"Artist">[]): ShelfCard[] {
  return artists.map((artist) => ({
    key: `artist-${artist.id}`,
    href: `/artists/${encodeURIComponent(artist.id)}`,
    title: artist.name,
    subtitle: artistCount(artist),
    cover: {
      kind: "artist",
      id: artist.id,
      recording: artist.artwork_recording_id ?? undefined,
      photo: artist.provider_photo === true,
    },
  }));
}
