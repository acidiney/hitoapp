import { router } from "@inertiajs/react";
import type { PlaylistPageProps } from "../../../shared/types";
import { useMutation } from "../../../shared/hooks/useMutation";
import EmptyState from "../../../shared/ui/EmptyState";
import CollectionHeader from "../../../shared/ui/CollectionHeader";
import Breadcrumbs from "../../../shared/ui/Breadcrumbs";
import { tintFor } from "../../../shared/ui/collectionTint";
import PlayShuffle from "../../playback/components/PlayShuffle";
import PlaylistRename from "../components/PlaylistRename";
import { usePlaylistEdit } from "../hooks/usePlaylistEdit";
import Artwork from "../../artwork/components/Artwork";
import ArtworkStatus from "../../artwork/components/ArtworkStatus";
import TrackTable, {
  type TrackTableRow,
} from "../../playback/components/TrackTable";
import TrackToolbar from "../../playback/components/TrackToolbar";
import PlaylistFollowStatus from "../components/PlaylistFollowStatus";
import PlaylistFollowResume from "../components/PlaylistFollowResume";
import PlaylistMetadataRefresh from "../components/PlaylistMetadataRefresh";
import PlaylistDownload from "../../downloads/components/PlaylistDownload";
import { usePlaylistFollow } from "../hooks/usePlaylistFollow";
import { useFollowResume } from "../hooks/useFollowResume";
import { usePlaylistMetadataRefresh } from "../hooks/usePlaylistMetadataRefresh";

export default function PlaylistPage({
  user,
  playlists,
  detail,
  native_playback,
}: PlaylistPageProps) {
  const { mutate, pending } = useMutation(user.id);
  const followed = detail.playlist.kind === "followed";
  const copied = detail.playlist.kind === "local" && detail.origin != null;
  const metadata = usePlaylistMetadataRefresh(
    user.id,
    copied ? detail.playlist.id : null,
    copied ? detail.origin : null,
  );
  const follow = usePlaylistFollow(
    user.id,
    followed ? detail.playlist.id : null,
    detail.follow_content_revision ?? null,
  );
  const resume = useFollowResume(
    user.id,
    followed ? detail.playlist.id : null,
    followed ? detail.origin : null,
    follow.retryStatus,
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
  const edit = usePlaylistEdit(user.id, detail.playlist.id, detail.entries);
  const editable = detail.playlist.kind === "local";
  const ready = detail.entries.filter(
    (entry) => entry.recording_id != null && entry.available,
  ).length;
  const stats = `${entryCount} ${entryCount === 1 ? "entry" : "entries"} · ${ready} ready${
    entryCount > ready ? ` · ${entryCount - ready} unavailable` : ""
  }`;
  return (
    <>
      <CollectionHeader
        tint={tintFor(detail.playlist.id)}
        size="playlist"
        crumbs={
          <Breadcrumbs
            crumbs={[
              { label: "Library", href: "/library" },
              { label: "Playlists", href: "/playlists" },
              { label: detail.playlist.name },
            ]}
          />
        }
        cover={
          <Artwork
            playlistId={copied ? detail.playlist.id : undefined}
            recording={copied ? undefined : tracks[0]?.id}
            fallback={{
              id: detail.playlist.id,
              title: detail.playlist.name,
              artist: "Playlist",
            }}
            playlist
          />
        }
        eyebrow={
          followed
            ? "Playlist · followed"
            : detail.origin
              ? "Playlist · imported copy"
              : "Playlist · yours"
        }
        title={detail.playlist.name}
        stats={stats}
        actions={
          <>
            <PlayShuffle tracks={tracks} label="playlist" />
            {editable && (
              <button
                type="button"
                className="button tint-secondary"
                aria-pressed={edit.editing}
                onClick={() => edit.setEditing(!edit.editing)}
              >
                {edit.editing ? "Done" : "Edit"}
              </button>
            )}
          </>
        }
      />
      {detail.origin && (
        <p className="origin-note">
          <span>{detail.origin.attribution.label}</span>
          {" · "}
          <span>
            {followed
              ? "Read-only playlist. Its provider updates its name, tracks and order. Tracks need an available source to play."
              : "Editable copy. Provider changes will not update it. Tracks need an available source to play."}
          </span>
        </p>
      )}
      {edit.editing && editable && (
        <PlaylistRename
          key={detail.playlist.name}
          name={detail.playlist.name}
          pending={edit.pending}
          save={edit.rename}
        />
      )}
      {copied && <ArtworkStatus playlistId={detail.playlist.id} />}
      <PlaylistDownload
        account={user.id}
        playlist={detail.playlist.id}
        local={native_playback}
      />
      {followed && <PlaylistFollowStatus {...follow} />}
      {copied && <PlaylistMetadataRefresh {...metadata} />}
      {followed && detail.origin && (
        <PlaylistFollowResume
          {...resume}
          paused={follow.follow?.status === "paused"}
        />
      )}
      {entryCount > 0 ? (
        <>
          <TrackToolbar tracks={tracks} label="playlist" withPlay={false}>
            {deletion}
          </TrackToolbar>
          <TrackTable
            rows={rows}
            playlists={playlists}
            playlist={detail.playlist}
            mutate={mutate}
            edit={{
              active: edit.editing && editable,
              busy: edit.pending,
              move: (id, delta) => void edit.move(id, delta),
            }}
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
