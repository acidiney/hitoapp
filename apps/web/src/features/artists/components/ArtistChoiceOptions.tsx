import { useState } from "react";
import type { Schema } from "../../../shared/schema";
import Button from "../../../shared/ui/Button";
import { evidence, plural } from "./ArtistMatchSuggestionList";

type Option = {
  key: string;
  name: string;
  reason: string;
  run: () => void;
};

export default function ArtistChoiceOptions({
  credit,
  suggestions,
  groups,
  busy,
  searchHref,
  onSuggestion,
  onGroup,
  onSearch,
  onNone,
  onCancel,
}: {
  credit: string;
  suggestions: Schema<"ArtistMatchSuggestion">[];
  groups: Schema<"ArtistIdentityGroup">[];
  busy: boolean;
  searchHref: string | null;
  onSuggestion: (group: Schema<"ArtistIdentityGroup">) => void;
  onGroup: (group: Schema<"ArtistIdentityGroup">) => void;
  onSearch: (href: string) => void;
  onNone: () => void;
  onCancel: () => void;
}) {
  const options: Option[] = [
    ...suggestions.map((item) => ({
      key: `suggestion-${item.group.id}`,
      name: item.group.name.value,
      reason: `You reviewed this artist · ${plural(item.undecided_tracks, "track", "tracks")} would use it · ${evidence(item)}`,
      run: () => onSuggestion(item.group),
    })),
    ...groups.map((group) => ({
      key: `group-${group.id}`,
      name: `${group.name.value} (this credit)`,
      reason: `Use this identity for all tracks credited ${credit} · ${plural(group.recording_count, "track", "tracks")}`,
      run: () => onGroup(group),
    })),
    ...(searchHref
      ? [
          {
            key: "search",
            name: "Search for another artist",
            reason: "Look for the artist in your services",
            run: () => onSearch(searchHref),
          },
        ]
      : []),
    ...(suggestions.length > 0
      ? [
          {
            key: "none",
            name: "None of these",
            reason: "Keep it unmatched",
            run: onNone,
          },
        ]
      : []),
  ];
  const [picked, setPicked] = useState<string | null>(null);
  const selected =
    options.find((option) => option.key === picked) ?? options[0];
  return (
    <>
      <fieldset className="choice-list" disabled={busy}>
        <legend className="sr-only">Which artist is {credit}?</legend>
        {options.map((option) => (
          <label key={option.key} className="choice-row">
            <input
              type="radio"
              name="artist-choice"
              checked={option.key === selected?.key}
              onChange={() => setPicked(option.key)}
            />
            <span className="choice-row-copy">
              <span>{option.name}</span>
              <span>{option.reason}</span>
            </span>
          </label>
        ))}
      </fieldset>
      <footer className="review-actions credit-dialog-actions">
        <Button disabled={busy} onClick={onCancel}>
          Cancel
        </Button>
        <Button
          variant="primary"
          disabled={busy || !selected}
          onClick={() => selected?.run()}
        >
          Continue
        </Button>
      </footer>
    </>
  );
}
