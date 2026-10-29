import type { Home, Playlist } from "../../shared/types";

export const QUICK_ACCESS_LIMIT = 6;
const RECENT_ALBUM_TILES = 3;
const LIKED_CAP = 1000;

export type QuickTile = {
  key: string;
  href: string;
  title: string;
  subtitle: string;
  kind: "release" | "playlist" | "likes";
  coverId: string;
  coverArtist?: string;
};

function trackCount(count: number): string {
  if (count >= LIKED_CAP) return `${LIKED_CAP.toLocaleString("en-US")}+ tracks`;
  return `${count} ${count === 1 ? "track" : "tracks"}`;
}

export function quickAccess(home: Home, playlists: Playlist[]): QuickTile[] {
  const tiles: QuickTile[] = home.recent_albums
    .slice(0, RECENT_ALBUM_TILES)
    .map((album) => ({
      key: `album-${album.id}`,
      href: `/albums/${album.id}`,
      title: album.title,
      subtitle: `Album · ${album.artist}`,
      kind: "release",
      coverId: album.id,
      coverArtist: album.artist,
    }));
  if (home.liked_count > 0)
    tiles.push({
      key: "likes",
      href: "/likes",
      title: "Liked tracks",
      subtitle: trackCount(home.liked_count),
      kind: "likes",
      coverId: "liked-tracks",
    });
  for (const playlist of playlists)
    tiles.push({
      key: `playlist-${playlist.id}`,
      href: `/playlists/${playlist.id}`,
      title: playlist.name,
      subtitle:
        playlist.kind === "followed" ? "Followed playlist" : "Your playlist",
      kind: "playlist",
      coverId: playlist.id,
    });
  for (const album of home.recent_albums.slice(RECENT_ALBUM_TILES))
    tiles.push({
      key: `album-${album.id}`,
      href: `/albums/${album.id}`,
      title: album.title,
      subtitle: `Album · ${album.artist}`,
      kind: "release",
      coverId: album.id,
      coverArtist: album.artist,
    });
  return tiles.slice(0, QUICK_ACCESS_LIMIT);
}

export function greeting(hour: number, name: string): string {
  const part =
    hour < 5 || hour >= 18 ? "evening" : hour < 12 ? "morning" : "afternoon";
  return `Good ${part}, ${name}`;
}
