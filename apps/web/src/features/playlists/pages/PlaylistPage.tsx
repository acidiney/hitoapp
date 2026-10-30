import type { Schema } from "../../../shared/schema";
import { router } from "@inertiajs/react";

import { useMutation } from "../../../shared/hooks/useMutation";
import EmptyState from "../../../shared/ui/EmptyState";
import CollectionHeader from "../../../shared/ui/CollectionHeader";
import Breadcrumbs from "../../../shared/ui/Breadcrumbs";
import { headerCoverId, tintFor } from "../../../shared/ui/collectionTint";
import PlayShuffle from "../../playback/components/PlayShuffle";
import PlaylistRename from "../components/PlaylistRename";
import { usePlaylistEdit } from "../hooks/usePlaylistEdit";
import Artwork from "../../artwork/components/Artwork";
import ArtworkStatus from "../../artwork/components/ArtworkStatus";
import TrackTable, {
  type TrackTableRow,
} from "../../playback/components/TrackTable";
import PlaylistFollowStatus from "../components/PlaylistFollowStatus";
import PlaylistFollowResume from "../components/PlaylistFollowResume";
import PlaylistMetadataRefresh from "../components/PlaylistMetadataRefresh";
import {
  PlaylistDownloadProvider,
  PlaylistDownloadButton,
  PlaylistDownloadStrip,
} from "../../downloads/components/PlaylistDownload";
import ActionMenu from "../../../shared/ui/ActionMenu";
import { usePlaylistFollow } from "../hooks/usePlaylistFollow";
import { useFollowResume } from "../hooks/useFollowResume";
import { usePlaylistMetadataRefresh } from "../hooks/usePlaylistMetadataRefresh";
import Button from "../../../shared/ui/Button";

export default function PlaylistPage({
  user,
  playlists,
  detail,
  native_playback,
}: Schema<"PlaylistPageProps">) {
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
  const readOnlyNotice =
    "Read-only playlist. Its provider updates its name, tracks and order. Tracks need an available source to play.";
  const edit = usePlaylistEdit(user.id, detail.playlist.id, detail.entries);
  const editable = detail.playlist.kind === "local";
  const ready = detail.entries.filter(
    (entry) => entry.recording_id != null && entry.available,
  ).length;
  const stats = `${entryCount} ${entryCount === 1 ? "entry" : "entries"} · ${ready} ready${
    entryCount > ready ? ` · ${entryCount - ready} unavailable` : ""
  }`;
  return (
    <PlaylistDownloadProvider account={user.id} playlist={detail.playlist.id}>
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
              id: headerCoverId(detail.playlist.id),
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
              <Button
                className="tint-secondary"
                type="button"
                pressed={edit.editing}
                onClick={() => edit.setEditing(!edit.editing)}
              >
                {edit.editing ? "Done" : "Edit"}
              </Button>
            )}
            <PlaylistDownloadButton />
            <ActionMenu
              label="More options"
              triggerClassName="tint-secondary"
              items={[
                {
                  label: "Delete playlist",
                  destructive: true,
                  disabled: pending,
                  onSelect: () => void remove(),
                },
              ]}
            />
          </>
        }
      />
      {detail.origin && !followed && (
        <p className="origin-note">
          <span>{detail.origin.attribution.label}</span>
          {" · "}
          <span>
            Editable copy. Provider changes will not update it. Tracks need an
            available source to play.
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
      {followed && (
        <PlaylistFollowStatus
          {...follow}
          attribution={detail.origin?.attribution.label ?? "Provider playlist"}
          notice={readOnlyNotice}
        />
      )}
      <PlaylistDownloadStrip local={native_playback} />
      {copied && <PlaylistMetadataRefresh {...metadata} />}
      {followed && detail.origin && (
        <PlaylistFollowResume
          {...resume}
          paused={follow.follow?.status === "paused"}
        />
      )}
      {entryCount > 0 ? (
        <>
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
    </PlaylistDownloadProvider>
  );
}
