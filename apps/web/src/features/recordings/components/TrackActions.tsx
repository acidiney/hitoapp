import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { Heart, ListPlus, Plus } from "lucide-react";
import ActionMenu from "../../../shared/ui/ActionMenu";
import Button from "../../../shared/ui/Button";
import { useMutation } from "../../../shared/hooks/useMutation";
import { useNotice } from "../../../shared/ui/Notices";
import { createUuid } from "../../../shared/uuid";
import { useArtwork } from "../../artwork/components/ArtworkProvider";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import type { Track } from "../../playback/model";

export default function TrackActions({
  account,
  track,
  liked: initiallyLiked,
  playlists,
}: {
  account: string;
  track: Track;
  liked: boolean;
  playlists: Schema<"Playlist">[];
}) {
  const player = usePlayer();
  const notice = useNotice();
  const { mutate, pending } = useMutation(account);
  const [liked, setLiked] = useState(initiallyLiked);
  const artwork = useArtwork("recording", track.id);
  const retryable =
    artwork.imageFailed &&
    ["ready", "failed", "cancelled", "unavailable"].includes(artwork.phase);
  const editable = playlists.filter((list) => list.kind === "local");

  async function toggleLike() {
    const next = !liked;
    const result = await mutate({
      kind: "like_set",
      recording_id: track.id,
      liked: next,
    });
    if (result.ok) setLiked(next);
  }

  async function addToPlaylist(playlist: Schema<"Playlist">) {
    const result = await mutate({
      kind: "entry_insert",
      playlist_id: playlist.id,
      entry_id: createUuid(),
      recording_id: track.id,
      position: Date.now(),
    });
    if (result.ok) notice(`Added to ${playlist.name}`);
  }

  return (
    <>
      <Button
        className="track-icon-action"
        pressed={liked}
        disabled={pending}
        aria-label={`${liked ? "Unlike" : "Like"} ${track.title}`}
        onClick={() => void toggleLike()}
      >
        <Heart
          size={18}
          fill={liked ? "currentColor" : "none"}
          aria-hidden="true"
        />
      </Button>
      {player.addToQueue && (
        <Button
          className="track-icon-action"
          aria-label={`Add ${track.title} to queue`}
          onClick={() => void player.addToQueue?.(track)}
        >
          <ListPlus size={18} aria-hidden="true" />
        </Button>
      )}
      <ActionMenu
        label={`Add ${track.title} to playlist`}
        triggerClassName="track-icon-action"
        icon={<Plus size={18} aria-hidden="true" />}
        items={
          editable.length > 0
            ? editable.map((list) => ({
                label: list.name,
                onSelect: () => void addToPlaylist(list),
              }))
            : [
                {
                  label: "No playlists yet",
                  onSelect: () => {},
                  disabled: true,
                },
              ]
        }
      />
      <ActionMenu
        label="More actions"
        triggerClassName="track-icon-action"
        items={[
          {
            label: "Retry artwork",
            onSelect: artwork.retry,
            disabled: !retryable,
          },
        ]}
      />
    </>
  );
}
