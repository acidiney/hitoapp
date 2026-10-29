import { Play, ArrowUpRight, ListMusic } from "lucide-react";
import { Link } from "@inertiajs/react";
import type { Playlist, Recording } from "../../../shared/types";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import Artwork from "../../artwork/components/Artwork";

export default function LibrarySpotlight({
  tracks,
  playlists,
}: {
  tracks: Recording[];
  playlists: Playlist[];
}) {
  const player = usePlayer();
  const index = Math.max(
    0,
    tracks.findIndex((track) => track.available),
  );
  const track = tracks[index];
  if (!track) return null;
  return (
    <div className="library-feature-row">
      <section className="library-spotlight" aria-label="Library spotlight">
        <Artwork
          recording={track.id}
          fallback={{
            id: track.release_id ?? track.id,
            title:
              track.album && track.album !== "Unknown album"
                ? track.album
                : track.title,
            artist: track.artist,
          }}
        />
        <div className="spotlight-copy">
          <span className="spotlight-label">From your collection</span>
          <h2>{track.title}</h2>
          <p>{track.artist}</p>
          {track.album && track.album !== "Unknown album" && (
            <span className="spotlight-album">{track.album}</span>
          )}
          <button
            className="button primary"
            onClick={() => void player.playQueue(tracks, index)}
          >
            <Play size={17} fill="currentColor" /> Play featured track
          </button>
        </div>
      </section>
      <section className="collection-shortcuts" aria-label="Your collections">
        <div className="section-heading">
          <h2>Made by you</h2>
          <Link href="/playlists" aria-label="View playlists">
            <ArrowUpRight size={18} />
          </Link>
        </div>
        {playlists.length ? (
          playlists.slice(0, 3).map((playlist) => (
            <Link key={playlist.id} href={`/playlists/${playlist.id}`}>
              <span className="collection-symbol">
                <ListMusic size={20} />
              </span>
              <span>
                <strong>{playlist.name}</strong>
                <small>
                  {playlist.kind === "followed"
                    ? "Followed playlist"
                    : "Personal playlist"}
                </small>
              </span>
              <ArrowUpRight size={15} />
            </Link>
          ))
        ) : (
          <>
            <p>Your own corner of the library.</p>
            <Link href="/playlists#new-playlist" className="text-link">
              Create your first playlist <ArrowUpRight size={16} />
            </Link>
          </>
        )}
      </section>
    </div>
  );
}
