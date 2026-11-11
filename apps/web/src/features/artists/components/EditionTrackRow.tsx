import type { ReactNode } from "react";
import { Play } from "lucide-react";
import ActionMenu from "../../../shared/ui/ActionMenu";

export type EditionRowState =
  | "library"
  | "absent"
  | "working"
  | "unplayable"
  | "attached";

const words: Record<EditionRowState, string> = {
  library: "In your library",
  absent: "Not in your library",
  working: "Working…",
  unplayable: "Not playable audio",
  attached: "Not in your library",
};

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
  play?: { label: string; disabled: boolean; onPlay: () => void };
  failure?: string;
  actions?: ReactNode;
}) {
  return (
    <li className="edition-row-item" data-state={state}>
      <p className="edition-row-number">{number}</p>
      <div className="edition-row-copy">
        <p className="edition-row-title">{title}</p>
        <p className="edition-row-status">
          <span className="edition-row-dot" aria-hidden="true" />
          <span className="edition-row-words">{words[state]}</span>
          {(detail || credit) && (
            <span className="edition-row-credit">
              {" · "}
              {detail ?? credit}
            </span>
          )}
        </p>
        {failure && <p role="alert">{failure}</p>}
      </div>
      <p className="edition-row-time">{time}</p>
      {play ? (
        <button
          type="button"
          className="edition-row-play"
          aria-label={play.label}
          disabled={play.disabled}
          onClick={play.onPlay}
        >
          <Play size={16} fill="currentColor" aria-hidden="true" />
        </button>
      ) : (
        <span className="edition-row-play" aria-hidden="true" />
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
                  label: play.label,
                  disabled: play.disabled,
                  onSelect: play.onPlay,
                },
              ]}
            />
          </div>
        ))}
    </li>
  );
}
