import type { Schema } from "../../../shared/schema";
import { fold } from "./ranking";

export type ArtistHit = {
  key: string;
  name: string;
  href: string;
  tracks: number;
  albums: number;
};

export function artistHits(
  groups: Schema<"ArtistIdentityGroup">[],
  credits: Schema<"Artist">[],
): ArtistHit[] {
  const chosen = new Set(groups.map((group) => fold(group.name.value)));
  return [
    ...groups.map((group) => ({
      key: `group-${group.id}`,
      name: group.name.value,
      href: `/artist-groups/${encodeURIComponent(group.id)}`,
      tracks: group.recording_count,
      albums: group.release_count,
    })),
    ...credits
      .filter((artist) => !chosen.has(fold(artist.name)))
      .map((artist) => ({
        key: `artist-${artist.id}`,
        name: artist.name,
        href: `/artists/${encodeURIComponent(artist.id)}`,
        tracks: artist.recording_count,
        albums: artist.release_count,
      })),
  ];
}
