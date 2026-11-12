import type { ReactNode } from "react";
import { Check } from "lucide-react";
import ActionMenu from "../../../shared/ui/ActionMenu";

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
}) {
  const isOwned = owned.includes(state);
  return (
    <li className="edition-row-item" data-state={state}>
      {play && (
        <button
          type="button"
          className="edition-row-open"
          aria-label={play.label}
          disabled={play.disabled}
          onClick={play.onPlay}
        />
      )}
      <p className="edition-row-number">{number}</p>
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
