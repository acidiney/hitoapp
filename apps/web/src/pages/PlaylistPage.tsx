import { router } from "@inertiajs/react";
import type { PlaylistPageProps } from "../types";
import { useMutation } from "../hooks/useMutation";
import EmptyState from "../components/EmptyState";
import Artwork from "../components/Artwork";
import TrackTable, { type TrackTableRow } from "../components/TrackTable";
import TrackToolbar from "../components/TrackToolbar";
import PlaylistFollowStatus from "../components/playlists/PlaylistFollowStatus";
import { usePlaylistFollow } from "../hooks/usePlaylistFollow";

export default function PlaylistPage({
  user,
  playlists,
  detail,
}: PlaylistPageProps) {
  const { mutate, pending } = useMutation(user.id);
  const followed = detail.playlist.kind === "followed";
  const follow = usePlaylistFollow(
    user.id,
    followed ? detail.playlist.id : null,
    detail.follow_content_revision ?? null,
  );
  const rows = detail.entries.map<TrackTableRow>((entry) => {
    const { recording_id, ...metadata } = entry;
    return recording_id == null
      ? {
          kind: "retained",
          entry: {
            entry_id: entry.entry_id,
            title: entry.title,
            artist: entry.artist,
            origin: entry.origin,
          },
        }
      : {
          kind: "recording",
          track: { ...metadata, id: recording_id, row_id: entry.entry_id },
        };
  });
  const tracks = rows.flatMap((row) =>
    row.kind === "recording" ? [row.track] : [],
  );
  const entryCount = detail.entries.length;
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
          <span>{followed ? "Followed playlist" : "Your playlist"}</span>
          <h1>{detail.playlist.name}</h1>
          <p>
            {entryCount} {entryCount === 1 ? "entry" : "entries"} ·{" "}
            {followed ? "Updated by its provider" : "Collected by you"}
          </p>
          {detail.origin && (
            <>
              <p>{detail.origin.attribution.label}</p>
              <p>
                {followed
                  ? "Read-only playlist. Its provider updates its name, tracks and order. Tracks need an available source to play."
                  : "Editable copy. Provider changes will not update it. Tracks need an available source to play."}
              </p>
            </>
          )}
        </div>
      </section>
      {followed && <PlaylistFollowStatus {...follow} />}
      {entryCount > 0 ? (
        <>
          <TrackToolbar
            tracks={tracks}
            label="playlist"
            description={`${entryCount} ${entryCount === 1 ? "entry" : "entries"}`}
          >
            {deletion}
          </TrackToolbar>
          <TrackTable
            rows={rows}
            playlists={playlists}
            playlist={detail.playlist}
            mutate={mutate}
          />
        </>
      ) : (
        <>
          <div className="list-toolbar">{deletion}</div>
          <EmptyState
            title={
              followed
                ? "The provider's playlist is empty"
                : "This playlist is ready for music"
            }
            text={
              followed
                ? "Hito will check for new entries on the next update."
                : "Add tracks using the playlist menu in your library."
            }
          />
        </>
      )}
    </>
  );
}
