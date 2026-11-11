import type { ReactNode } from "react";
import type { Schema } from "../../../shared/schema";
import { router } from "@inertiajs/react";
import ActionMenu, { type ActionMenuItem } from "../../../shared/ui/ActionMenu";
import { useNoticeIfProvided } from "../../../shared/ui/Notices";
import { createUuid } from "../../../shared/uuid";
import type { Mutate } from "../../../shared/hooks/useMutation";
import { log } from "../../../shared/log";
import { trackTabHref } from "../../recordings/model/trackTabs";
import { usePlayer } from "./PlaybackProvider";
import type { TrackRow } from "./TrackTable";

export function trackHref(recording: string): string {
  return `/recordings/${encodeURIComponent(recording)}`;
}

export function sourceHref(track: Pick<TrackRow, "id" | "available">): string {
  return track.available
    ? trackTabHref(track.id, "sources")
    : `${trackHref(track.id)}#find-audio`;
}

export default function TrackRowMenu({
  track,
  playlists,
  playlist,
  mutate,
  onMutationAccepted,
  queueActions = true,
  onNavigate,
  label,
  icon,
  leading = [],
  triggerClassName = "icon-button",
}: {
  label?: string;
  icon?: ReactNode;
  leading?: ActionMenuItem[];
  triggerClassName?: string;
  track: TrackRow;
  playlists: Schema<"Playlist">[];
  playlist?: Schema<"Playlist">;
  mutate: Mutate;
  onMutationAccepted?: (action: Schema<"Action">) => void;
  queueActions?: boolean;
  onNavigate?: () => void;
}) {
  const player = usePlayer();
  const notify = useNoticeIfProvided();
  const editable = playlists.filter((list) => list.kind === "local");
  const entryId = track.entry_id;

  async function update(action: Schema<"Action">, done: string) {
    if (!(await mutate(action)).ok) return;
    notify(done);
    if (onMutationAccepted) onMutationAccepted(action);
    else router.reload();
  }

  function visit(href: string) {
    onNavigate?.();
    router.visit(href);
  }

  const queueItems: ActionMenuItem[] = [];
  if (queueActions && player.playNext)
    queueItems.push({
      label: "Play next",
      onSelect: () => {
        log.info("track.play_next", { recording_id: track.id });
        void player.playNext?.(track);
      },
    });
  if (queueActions && player.addToQueue)
    queueItems.push({
      label: "Add to queue",
      onSelect: () => {
        log.info("track.add_to_queue", { recording_id: track.id });
        void player.addToQueue?.(track);
      },
    });

  const items: ActionMenuItem[] = [
    ...leading,
    {
      label: "Track details",
      onSelect: () => visit(trackHref(track.id)),
    },
    ...queueItems,
    {
      label: "Add to playlist",
      items:
        editable.length > 0
          ? editable.map((list) => ({
              label: list.name,
              onSelect: () =>
                void update(
                  {
                    kind: "entry_insert",
                    playlist_id: list.id,
                    entry_id: createUuid(),
                    recording_id: track.id,
                    position: Date.now(),
                  },
                  `Added to ${list.name}`,
                ),
            }))
          : [{ label: "No playlists yet", disabled: true }],
    },
    ...(playlist?.kind === "local" && entryId
      ? [
          {
            label: "Remove from playlist",
            destructive: true,
            onSelect: () =>
              void update(
                {
                  kind: "entry_remove",
                  playlist_id: playlist.id,
                  entry_id: entryId,
                },
                `Removed from ${playlist.name}`,
              ),
          },
        ]
      : []),
    {
      label: track.available ? "Fix source" : "Find audio",
      onSelect: () => visit(sourceHref(track)),
    },
  ];

  return (
    <ActionMenu
      label={label ?? `Actions for ${track.title}`}
      triggerClassName={triggerClassName}
      icon={icon}
      align="end"
      items={items}
    />
  );
}
