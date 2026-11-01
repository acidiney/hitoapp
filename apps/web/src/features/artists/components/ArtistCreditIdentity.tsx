import { Link } from "@inertiajs/react";
import { useEffect, useRef, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { useArtistCreditIdentity } from "../hooks/useArtistCreditIdentity";
import { useArtistMatchSuggestions } from "../hooks/useArtistMatchSuggestions";
import { useArtistMatchingStatus } from "../hooks/useArtistMatchingStatus";
import ArtistMatchSuggestionList from "./ArtistMatchSuggestionList";
import Button from "../../../shared/ui/Button";
import { chosenEvidenceText } from "../model/associationEvidence";

function plural(count: number, one: string, many: string) {
  return `${count} ${count === 1 ? one : many}`;
}

function provenance(chosen: Schema<"ArtistMatchChosen">) {
  if (chosen.automatic_tracks === 0) return "chosen by you";
  if (chosen.automatic_tracks === chosen.tracks) return "matched automatically";
  return `${chosen.automatic_tracks} of ${chosen.tracks} tracks matched automatically`;
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
  const { status } = useArtistMatchingStatus(account);
  const [choosing, setChoosing] = useState(false);
  const { refresh } = suggested;
  useEffect(() => setChoosing(false), [artist.id]);
  useEffect(() => {
    if (identity.totals) void refresh();
  }, [identity.totals, refresh]);
  const working = !!status?.enabled && status.state !== "idle";
  const wasWorking = useRef(false);
  useEffect(() => {
    if (wasWorking.current && !working) void refresh();
    wasWorking.current = working;
  }, [working, refresh]);
  const busy = identity.step === "loading" || identity.step === "applying";
  const read = suggested.suggestions;
  const matched = read !== null && read.chosen.length > 0;
  const open = () => {
    setChoosing(true);
    identity.find();
  };
  return (
    <section aria-label="Artist identity for this credit" aria-busy={busy}>
      <h2 className="discography-heading">Artist identity</h2>
      {read && read.chosen.length > 0 ? (
        read.chosen.map((chosen) => {
          const how = chosenEvidenceText(chosen.evidence);
          return (
            <div key={chosen.group.id}>
              <p aria-live="polite">
                Artist: {chosen.group.name.value}
                {read.chosen.length > 1 &&
                  ` · ${plural(chosen.tracks, "track", "tracks")}`}{" "}
                · {provenance(chosen)}{" "}
                <Link
                  className="text-link"
                  href={`/artist-groups/${encodeURIComponent(chosen.group.id)}`}
                >
                  Open artist
                </Link>
              </p>
              {how && <p className="muted">{how}</p>}
            </div>
          );
        })
      ) : working ? (
        <p aria-live="polite">Hito is identifying this artist…</p>
      ) : (
        read && <p aria-live="polite">Not identified yet</p>
      )}
      {matched && read && read.undecided_tracks > 0 && (
        <p className="muted">
          {plural(read.undecided_tracks, "track", "tracks")} of this credit not
          matched yet.
        </p>
      )}
      {!choosing && (!matched || (read?.undecided_tracks ?? 0) > 0) && (
        <Button type="button" aria-expanded={false} onClick={open}>
          Choose the artist
        </Button>
      )}
      {choosing && (
        <div role="group" aria-label="Choose the artist">
          <p>
            Use an artist you already reviewed for another track. Equal names
            alone never merge artists.
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
          {identity.step === "choosing" && identity.groups.length === 0 && (
            <p>
              No reviewed artist matches this credit yet. Review an artist on
              one of its tracks first.
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
        </div>
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
