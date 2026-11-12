import { useMemo } from "react";
import ActionMenu, { type ActionMenuItem } from "../../../shared/ui/ActionMenu";
import type { Track } from "../model";
import { DotsIcon } from "./QueueIcons";
import TrackRowMenu from "./TrackRowMenu";
import { useTrackActions } from "./TrackActionsProvider";

type Props = {
  track: Track;
  canMoveUp: boolean;
  canMoveDown: boolean;
  moveUp(): void;
  moveDown(): void;
  remove(): void;
};

export default function QueueRowMenu({
  track,
  canMoveUp,
  canMoveDown,
  moveUp,
  moveDown,
  remove,
}: Props) {
  const actions = useTrackActions();
  const row = useMemo(
    () => ({ ...track, row_id: track.id, liked: track.liked ?? false }),
    [track],
  );
  const label = `Actions for ${track.title}: move up, move down, remove`;
  const leading: ActionMenuItem[] = [
    { label: "Move up", disabled: !canMoveUp, onSelect: moveUp },
    { label: "Move down", disabled: !canMoveDown, onSelect: moveDown },
    { label: "Remove", destructive: true, onSelect: remove },
  ];
  if (!actions)
    return (
      <ActionMenu
        label={label}
        items={leading}
        icon={<DotsIcon />}
        triggerClassName="queue-menu"
        align="end"
      />
    );
  return (
    <TrackRowMenu
      track={row}
      playlists={actions.playlists}
      mutate={actions.mutate}
      queueActions={false}
      label={label}
      icon={<DotsIcon />}
      leading={leading}
      triggerClassName="queue-menu"
    />
  );
}
