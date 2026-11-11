import type { Schema } from "../../../shared/schema";

import Button from "../../../shared/ui/Button";

function plural(count: number, one: string, many: string) {
  return `${count} ${count === 1 ? one : many}`;
}

function evidence(item: Schema<"ArtistMatchSuggestion">) {
  const parts = ["same name"];
  if (item.shared_albums > 0)
    parts.push(plural(item.shared_albums, "shared album", "shared albums"));
  if (item.same_length_tracks > 0)
    parts.push(
      plural(
        item.same_length_tracks,
        "track with the same title and length",
        "tracks with the same title and length",
      ),
    );
  return parts.length === 1 ? "same name only" : parts.join(", ");
}

export default function ArtistMatchSuggestionList({
  items,
  dismissed,
  busy,
  onChoose,
  onDismiss,
  onRestore,
}: {
  items: Schema<"ArtistMatchSuggestion">[];
  dismissed: Schema<"ArtistMatchSuggestion">[];
  busy: boolean;
  onChoose: (group: Schema<"ArtistIdentityGroup">) => void;
  onDismiss: (group: string) => void;
  onRestore: (group: string) => void;
}) {
  if (items.length === 0 && dismissed.length === 0) return null;
  return (
    <section
      className="identity-suggestions"
      aria-label="Suggested artists for this credit"
      aria-busy={busy}
    >
      {items.length > 0 && (
        <>
          <h3 className="identity-heading">
            Looks like an artist you reviewed
          </h3>
          <p>
            These are suggestions only. Nothing changes until you review and
            confirm.
            {dismissed.length > 0
              ? ` ${plural(dismissed.length, "suggestion is", "suggestions are")} dismissed.`
              : ""}
          </p>
          <ul className="identity-list">
            {items.map((item) => (
              <li key={item.group.id}>
                <span className="identity-list-copy">
                  <strong>{item.group.name.value}</strong>
                  <span>
                    {plural(item.undecided_tracks, "track", "tracks")} here
                    would use it · {evidence(item)}
                  </span>
                </span>
                <span className="identity-list-actions">
                  <Button
                    type="button"
                    disabled={busy}
                    aria-label={`Review suggested artist: ${item.group.name.value}`}
                    onClick={() => onChoose(item.group)}
                  >
                    Review suggestion
                  </Button>
                  <Button
                    type="button"
                    variant="link"
                    disabled={busy}
                    aria-label={`Dismiss suggested artist: ${item.group.name.value}`}
                    onClick={() => onDismiss(item.group.id)}
                  >
                    Not the same artist
                  </Button>
                </span>
              </li>
            ))}
          </ul>
        </>
      )}
      {dismissed.length > 0 && (
        <details>
          <summary>Dismissed suggestions ({dismissed.length})</summary>
          <ul className="identity-list">
            {dismissed.map((item) => (
              <li key={item.group.id}>
                <span className="identity-list-copy">
                  <strong>{item.group.name.value}</strong>
                  <span>
                    {plural(item.undecided_tracks, "track", "tracks")} here
                    would use it · {evidence(item)}
                  </span>
                </span>
                <span className="identity-list-actions">
                  <Button
                    type="button"
                    disabled={busy}
                    aria-label={`Restore suggested artist: ${item.group.name.value}`}
                    onClick={() => onRestore(item.group.id)}
                  >
                    Restore
                  </Button>
                </span>
              </li>
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}
