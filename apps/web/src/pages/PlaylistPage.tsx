import { router } from "@inertiajs/react";
import type { PlaylistPageProps } from "../types";
import { useMutation } from "../hooks/useMutation";
import EmptyState from "../components/EmptyState";
import Artwork from "../components/Artwork";
import TrackTable from "../components/TrackTable";
import TrackToolbar from "../components/TrackToolbar";

export default function PlaylistPage({
  user,
  playlists,
  detail,
}: PlaylistPageProps) {
  const { mutate, pending } = useMutation(user.id);
  const tracks = detail.entries.map((entry) => ({
    ...entry,
    id: entry.recording_id,
    row_id: entry.entry_id,
  }));
  async function remove() {
    const result = await mutate({
      kind: "playlist_delete",
      playlist_id: detail.playlist.id,
    });
    if (result.ok) router.visit("/playlists");
  }
  const deletion = (
    <button
      className="button danger"
      disabled={pending}
      onClick={() => void remove()}
    >
      Delete playlist
    </button>
  );
  return (
    <>
      <section className="collection-hero">
        <Artwork recording={tracks[0]?.id} playlist />
        <div>
          <span>
            {detail.playlist.kind === "followed"
              ? "Followed playlist"
              : "Your playlist"}
          </span>
          <h1>{detail.playlist.name}</h1>
          <p>
            {tracks.length} tracks ·{" "}
            {detail.playlist.kind === "followed"
              ? "Updated by its provider"
              : "Collected by you"}
          </p>
        </div>
      </section>
      {tracks.length > 0 ? (
        <>
          <TrackToolbar
            tracks={tracks}
            label="playlist"
            description={`${tracks.length} tracks`}
          >
            {deletion}
          </TrackToolbar>
          <TrackTable
            tracks={tracks}
            playlists={playlists}
            playlist={detail.playlist}
            mutate={mutate}
          />
        </>
      ) : (
        <>
          <div className="list-toolbar">{deletion}</div>
          <EmptyState
            title="This playlist is ready for music"
            text="Add tracks using the playlist menu in your library."
          />
        </>
      )}
    </>
  );
}
