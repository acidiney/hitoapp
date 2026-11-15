import type { ReactNode } from "react";
import { Check, Plus } from "lucide-react";
import type { Schema } from "../../../shared/schema";
import ActionMenu, { type ActionMenuItem } from "../../../shared/ui/ActionMenu";
import type { RowPlayState } from "../../playback/hooks/useNowPlaying";
import {
  PlayingBars,
  PlayingStatus,
} from "../../playback/components/PlayingBars";

export type EditionRowState =
  | "ready"
  | "library"
  | "absent"
  | "working"
  | "unplayable"
  | "attached";

const words: Record<EditionRowState, string> = {
  ready: "Ready",
  library: "In your library",
  absent: "Not in your library",
  working: "Working…",
  unplayable: "Not playable audio",
  attached: "Not in your library",
};

const owned: EditionRowState[] = ["ready", "library"];

export function ownedRowState(recording: Schema<"Recording">): {
  state: EditionRowState;
  detail?: string;
} {
  if (recording.available) return { state: "ready" };
  if (recording.plays_from_services)
    return { state: "ready", detail: "from your services" };
  return { state: "library" };
}

export default function EditionTrackRow({
  number,
  title,
  state,
  detail,
  credit,
  time,
  play,
  failure,
  actions,
  add,
  more = [],
  playing = null,
}: {
  number: ReactNode;
  title: string;
  state: EditionRowState;
  detail?: string;
  credit?: string | null;
  time: ReactNode;
  play?: {
    label: string;
    menuLabel: string;
    disabled: boolean;
    onPlay: () => void;
  };
  failure?: string;
  actions?: ReactNode;
  add?: { label: string; disabled: boolean; onAdd: () => void };
  more?: ActionMenuItem[];
  playing?: RowPlayState;
}) {
  const isOwned = owned.includes(state);
  return (
    <li
      className="edition-row-item"
      data-state={state}
      aria-current={playing ? "true" : undefined}
    >
      {play && (
        <button
          type="button"
          className="edition-row-open"
          aria-label={play.label}
          disabled={play.disabled}
          onClick={play.onPlay}
        />
      )}
      <p className="edition-row-number">
        {playing ? (
          <>
            <PlayingBars state={playing} />
            <PlayingStatus state={playing} />
          </>
        ) : (
          number
        )}
      </p>
      <p className="edition-row-title">{title}</p>
      <p className="edition-row-sub">
        <span className="edition-row-status">
          <span className="edition-row-dot" aria-hidden="true" />
          <span>
            {words[state]}
            {detail ? ` · ${detail}` : ""}
          </span>
        </span>
        {credit && <span className="edition-row-credit">{credit}</span>}
      </p>
      <p className="edition-row-time">{time}</p>
      <div className="edition-row-controls">
        {isOwned && (
          <span
            className="edition-row-check"
            role="img"
            aria-label="In your library"
          >
            <Check size={16} strokeWidth={2.5} aria-hidden="true" />
          </span>
        )}
        {add && (
          <button
            type="button"
            className="icon-button edition-row-add"
            aria-label={add.label}
            title="Add to your library"
            disabled={add.disabled}
            onClick={add.onAdd}
          >
            <Plus size={16} aria-hidden="true" />
          </button>
        )}
        {actions ??
          (play && (
            <div className="track-actions">
              <ActionMenu
                label={`More actions for ${title}`}
                triggerClassName="icon-button"
                align="end"
                items={[
                  {
                    label: play.menuLabel,
                    disabled: play.disabled,
                    onSelect: play.onPlay,
                  },
                  ...more,
                ]}
              />
            </div>
          ))}
      </div>
      {failure && (
        <p className="edition-row-failure" role="alert">
          {failure}
        </p>
      )}
    </li>
  );
}
