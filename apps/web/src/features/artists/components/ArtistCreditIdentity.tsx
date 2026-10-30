import { Link } from "@inertiajs/react";
import { useEffect } from "react";
import { useArtistCreditIdentity } from "../hooks/useArtistCreditIdentity";
import { useArtistMatchSuggestions } from "../hooks/useArtistMatchSuggestions";
import ArtistMatchSuggestionList from "./ArtistMatchSuggestionList";
import Button from "../../../shared/ui/Button";

function plural(count: number, one: string, many: string) {
  return `${count} ${count === 1 ? one : many}`;
}

export default function ArtistCreditIdentity({
  account,
  artist,
}: {
  account: string;
  artist: { id: string; name: string };
}) {
  const identity = useArtistCreditIdentity(account, artist);
  const suggested = useArtistMatchSuggestions(account, artist.id);
  const { refresh } = suggested;
  useEffect(() => {
    if (identity.totals) void refresh();
  }, [identity.totals, refresh]);
  const busy = identity.step === "loading" || identity.step === "applying";
  return (
    <section aria-label="Artist identity for this credit" aria-busy={busy}>
      <h2 className="discography-heading">Artist identity</h2>
      <p>
        Use an artist you already reviewed for another track. Equal names alone
        never merge artists.
      </p>
      {(identity.step === "closed" || identity.step === "choosing") &&
        suggested.suggestions && (
          <ArtistMatchSuggestionList
            items={suggested.suggestions.items}
            dismissed={suggested.suggestions.dismissed_items}
            busy={suggested.busy}
            onChoose={identity.choose}
            onDismiss={(group) => void suggested.dismiss(group)}
            onRestore={(group) => void suggested.restore(group)}
          />
        )}
      {identity.step === "closed" && (
        <Button type="button" onClick={identity.find}>
          Find reviewed artists for this credit
        </Button>
      )}
      {identity.step === "choosing" && identity.groups.length === 0 && (
        <p>
          No reviewed artist matches this credit yet. Review an artist on one of
          its tracks first.
        </p>
      )}
      {identity.step === "choosing" && (
        <ul>
          {identity.groups.map((group) => (
            <li key={group.id}>
              {group.name.value} ·{" "}
              {plural(group.recording_count, "track", "tracks")}{" "}
              <Button
                type="button"
                aria-label={`Use this identity for all tracks with this credit: ${group.name.value}`}
                onClick={() => identity.choose(group)}
              >
                Use this identity for all tracks with this credit
              </Button>
            </li>
          ))}
        </ul>
      )}
      {identity.step === "confirming" && identity.selected && (
        <div role="group" aria-label="Confirm artist identity">
          <p>
            Use {identity.selected.name.value} for every track you can access
            under the credit {artist.name}? A track keeps its choice when it
            already has a different reviewed artist, or when a different
            artist's choice needs review. Tracks never chosen, reset, or needing
            review for this artist are updated. You can reset any track later.
          </p>
          <Button
            type="button"
            onClick={() => identity.apply(identity.selected!)}
          >
            Confirm
          </Button>
          <Button type="button" onClick={identity.cancel}>
            Cancel
          </Button>
        </div>
      )}
      {identity.step === "applying" && <p>Applying the artist identity…</p>}
      {identity.error && <p role="alert">{identity.error}</p>}
      {identity.retrying && (
        <Button type="button" disabled={busy} onClick={identity.retry}>
          Retry the same artist identity request
        </Button>
      )}
      {identity.totals && identity.selected && (
        <div role="status">
          <p>
            {plural(
              identity.totals.applied,
              "track now uses",
              "tracks now use",
            )}{" "}
            this artist.
          </p>
          {identity.totals.unchanged > 0 && (
            <p>{identity.totals.unchanged} already used it.</p>
          )}
          {identity.totals.skippedDifferent > 0 && (
            <p>
              {identity.totals.skippedDifferent} kept a different reviewed
              artist.
            </p>
          )}
          {identity.totals.skippedUnavailable > 0 && (
            <p>{identity.totals.skippedUnavailable} could not be changed.</p>
          )}
          <Link
            className="text-link"
            href={`/artist-groups/${encodeURIComponent(identity.selected.id)}`}
          >
            Open chosen artist
          </Link>
        </div>
      )}
    </section>
  );
}
