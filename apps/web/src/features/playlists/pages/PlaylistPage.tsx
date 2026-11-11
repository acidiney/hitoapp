import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { router } from "@inertiajs/react";

import { useMutation } from "../../../shared/hooks/useMutation";
import EmptyState from "../../../shared/ui/EmptyState";
import CollectionHeader from "../../../shared/ui/CollectionHeader";
import Breadcrumbs from "../../../shared/ui/Breadcrumbs";
import { headerCoverId, tintFor } from "../../../shared/ui/collectionTint";
import PlayShuffle from "../../playback/components/PlayShuffle";
import PlaylistEditPanel from "../components/PlaylistEditPanel";
import DiscardEditsDialog from "../components/DiscardEditsDialog";
import { usePlaylistEdit } from "../hooks/usePlaylistEdit";
import Artwork from "../../artwork/components/Artwork";
import PlaylistMosaic, {
  mosaicCovers,
} from "../../artwork/components/PlaylistMosaic";
import ArtworkStatus from "../../artwork/components/ArtworkStatus";
import { useArtworkRetryItem } from "../../artwork/components/ArtworkRetryMenu";
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

function rowsFor(entries: Schema<"PlaylistEntry">[]): TrackTableRow[] {
  return entries.map<TrackTableRow>((entry) => {
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
}

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
    follow.follow,
    follow.retryStatus,
  );
  const edit = usePlaylistEdit(
    user.id,
    detail.playlist.id,
    detail.playlist.name,
    detail.entries,
  );
  const [discarding, setDiscarding] = useState(false);
  const artworkRetry = useArtworkRetryItem({ playlistId: detail.playlist.id });
  const savedRows = rowsFor(detail.entries);
  const rows = edit.editing ? rowsFor(edit.entries) : savedRows;
  const tracks = savedRows.flatMap((row) =>
    row.kind === "recording" ? [row.track] : [],
  );
  const covers = mosaicCovers(tracks);
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
          !copied && covers.length > 1 ? (
            <PlaylistMosaic covers={covers} />
          ) : (
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
          )
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
            {editable && !edit.editing && (
              <Button
                className="tint-secondary"
                type="button"
                onClick={edit.begin}
              >
                Edit
              </Button>
            )}
            {editable && edit.editing && (
              <>
                <Button
                  variant="primary"
                  disabled={edit.pending || edit.name.trim() === ""}
                  onClick={() => void edit.done()}
                >
                  Done
                </Button>
                <Button
                  className="tint-secondary"
                  disabled={edit.pending}
                  onClick={() =>
                    edit.dirty ? setDiscarding(true) : edit.cancel()
                  }
                >
                  Cancel
                </Button>
              </>
            )}
            <PlaylistDownloadButton />
            <ActionMenu
              label="More options"
              triggerClassName="tint-secondary"
              items={[
                ...(artworkRetry ? [artworkRetry] : []),
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
        <PlaylistEditPanel
          playlistId={detail.playlist.id}
          playlists={playlists}
          name={edit.name}
          dirty={edit.dirty}
          disabled={edit.pending}
          onName={edit.setName}
        />
      )}
      {discarding && (
        <DiscardEditsDialog
          onKeep={() => setDiscarding(false)}
          onDiscard={() => {
            setDiscarding(false);
            edit.cancel();
          }}
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
        <PlaylistFollowResume {...resume} follow={follow.follow} />
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
              removed: edit.removed,
              move: edit.move,
              toggleRemove: edit.toggleRemove,
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
