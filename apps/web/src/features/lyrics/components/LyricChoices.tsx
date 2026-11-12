import Button from "../../../shared/ui/Button";
import {
  identityKey,
  identityNames,
  isRemovable,
  lyricIdentityLimit,
  type LyricIdentity,
} from "../model/lyricIdentity";
import type { LyricFailure } from "../model/lyricFailure";
import LyricState from "./LyricState";

export type LyricChoicesProps = {
  items: LyricIdentity[];
  selected: LyricIdentity | null;
  atLimit: boolean;
  removing: string | null;
  removalFailure: (LyricFailure & { key: string }) | null;
  onSelect: (key: string) => void;
  onRemove: (identity: LyricIdentity) => void;
};

export default function LyricChoices({
  items,
  selected,
  atLimit,
  removing,
  removalFailure,
  onSelect,
  onRemove,
}: LyricChoicesProps) {
  const names = identityNames(items);
  const selectedKey = selected && identityKey(selected);
  return (
    <fieldset className="lyric-choices" disabled={removing !== null}>
      <legend>Lyrics from</legend>
      <ul>
        {items.map((identity) => {
          const key = identityKey(identity);
          const name = names[key];
          return (
            <li key={key} data-selected={key === selectedKey || undefined}>
              <label>
                <input
                  type="radio"
                  name="lyric-choice"
                  checked={key === selectedKey}
                  disabled={!identity.selectable}
                  onChange={() => onSelect(key)}
                />
                <span className="lyric-choice-name">{name}</span>
                {identity.selectable ? (
                  <span className="lyric-choice-origin">
                    {isRemovable(identity)
                      ? "Chosen by you"
                      : "From this track's details"}
                  </span>
                ) : (
                  <LyricState tone="failed">
                    Service unavailable. Reconnect it to read these lyrics.
                  </LyricState>
                )}
              </label>
              {isRemovable(identity) && (
                <Button
                  className="lyric-choice-remove"
                  aria-label={`Remove ${name}`}
                  disabled={removing !== null}
                  onClick={() => onRemove(identity)}
                >
                  {removing === key ? "Removing…" : "Remove"}
                </Button>
              )}
              {removalFailure?.key === key && (
                <p role="alert" className="lyric-choice-error">
                  {removalFailure.message}
                </p>
              )}
            </li>
          );
        })}
      </ul>
      {atLimit && (
        <p className="lyric-limit" role="note">
          This track keeps up to {lyricIdentityLimit} lyric choices, including
          unavailable ones. Remove one to add another.
        </p>
      )}
    </fieldset>
  );
}
