import { artistHref, albumHref } from "../components/TrackNames";
import type { Track } from "../model";
import { useArtistGroupLink } from "./useArtistLink";

export type PlayerLink = { label: string; href: string };

export function libraryArtists(track: Track): PlayerLink[] {
  const seen = new Set<string>();
  const credited = (track.credited_artists ?? []).filter(
    (member) => !seen.has(member.id) && seen.add(member.id),
  );
  if (credited.length > 0)
    return credited.map((member) => ({
      label: member.name,
      href: `/artists/${encodeURIComponent(member.id)}`,
    }));
  const href = artistHref(track);
  return href ? [{ label: track.artist, href }] : [];
}

export function usePlayerLinks(account: string, track: Track | null) {
  const known = track ? libraryArtists(track) : [];
  const matched = useArtistGroupLink(
    account,
    track && known.length === 0 ? track : null,
  );
  const artists =
    known.length > 0 || !track || !matched
      ? known
      : [{ label: track.artist, href: matched }];
  return { artists, album: track ? albumHref(track) : null };
}
