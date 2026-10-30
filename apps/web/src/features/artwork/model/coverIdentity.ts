import type { Track } from "../../playback/model";
import type { CoverIdentity } from "../components/Artwork";

export function coverIdentity(track: Track | null): CoverIdentity | null {
  if (!track) return null;
  return {
    id: track.release_id ?? track.id,
    title: track.album ?? track.title,
    artist: track.artist,
  };
}
