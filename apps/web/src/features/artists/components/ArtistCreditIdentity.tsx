import { Link, router } from "@inertiajs/react";
import { useEffect, useRef, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { useArtistCreditIdentity } from "../hooks/useArtistCreditIdentity";
import { useArtistMatchSuggestions } from "../hooks/useArtistMatchSuggestions";
import { useArtistMatchingStatus } from "../hooks/useArtistMatchingStatus";
import { useCreditTailOffer } from "../hooks/useCreditTailOffer";
import ArtistMatchSuggestionList from "./ArtistMatchSuggestionList";
import ArtistChoiceOptions from "./ArtistChoiceOptions";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import { artistSearchHref } from "../model/artistSearchLink";
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
  searchRecording,
}: {
  account: string;
  artist: { id: string; name: string };
  searchRecording?: string;
}) {
  const identity = useArtistCreditIdentity(account, artist);
  const suggested = useArtistMatchSuggestions(account, artist.id);
  const { status } = useArtistMatchingStatus(account);
  const [choosing, setChoosing] = useState(false);
  const { refresh } = suggested;
  const opener = useRef<HTMLButtonElement>(null);
  useEffect(() => setChoosing(false), [artist.id]);
  useEffect(() => {
    if (identity.step === "closed" && identity.totals) setChoosing(false);
  }, [identity.step, identity.totals]);
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
  const identified =
    read !== null && read.chosen.length === 1 && read.undecided_tracks === 0
      ? read.chosen[0].group.id
      : null;
  useEffect(() => {
    if (identified)
      router.visit(`/artist-groups/${encodeURIComponent(identified)}`, {
        replace: true,
      });
  }, [identified]);
  const suggestionItems = suggested.suggestions?.items ?? [];
  const showChoices =
    (identity.step === "closed" || identity.step === "choosing") &&
    (identity.step === "choosing" ||
      suggestionItems.length > 0 ||
      !!searchRecording);
  const closeChoosing = () => {
    setChoosing(false);
    identity.cancel();
  };
  const partly = matched && (read?.undecided_tracks ?? 0) > 0;
  const tail = useCreditTailOffer(
    account,
    artist.id,
    partly && read ? read.chosen : null,
    refresh,
  );
  const offer = partly ? tail.offer : null;
  const open = () => {
    setChoosing(true);
    identity.find();
  };
  return (
    <section
      className="detail-section"
      aria-label="Artist identity for this credit"
      aria-busy={busy}
    >
      <h2>Artist identity</h2>
      {read && read.chosen.length > 0 ? (
        read.chosen.map((chosen) => {
          const how = chosenEvidenceText(chosen.evidence);
          return (
            <div key={chosen.group.id}>
              <p aria-live="polite">
                {partly ? "Also listed as" : "Artist:"}{" "}
                <span className="identity-name" title={chosen.group.name.value}>
                  {chosen.group.name.value}
                </span>
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
      ) : (
        <p className="identity-row" aria-live="polite">
          {working ? (
            <span>Hito is identifying this artist…</span>
          ) : (
            read && <span>Not identified yet</span>
          )}
          {(working || read) && <span aria-hidden="true">·</span>}
          <button
            ref={opener}
            id="choose-artist"
            type="button"
            className="text-action"
            onClick={open}
          >
            Choose the artist
          </button>
        </p>
      )}
      {matched && read && read.undecided_tracks > 0 && (
        <p className="muted">
          {plural(read.undecided_tracks, "track", "tracks")} of this credit not
          matched yet.
        </p>
      )}
      {partly && offer && (
        <>
          <div className="identity-actions">
            <Button
              variant="primary"
              disabled={tail.pending || busy}
              onClick={() => void tail.confirm()}
            >
              {offer.undecidedTracks === 1
                ? `Use ${offer.group.name.value} for the other track`
                : `Use ${offer.group.name.value} for the other ${offer.undecidedTracks} tracks`}
            </Button>
            <button
              ref={opener}
              id="choose-artist"
              type="button"
              className="text-action"
              disabled={tail.pending}
              onClick={open}
            >
              Choose a different artist
            </button>
          </div>
          {tail.pending && (
            <p role="status" className="muted">
              Applying the artist to the other tracks…
            </p>
          )}
          {tail.error && <p role="alert">{tail.error}</p>}
        </>
      )}
      {partly && !offer && (
        <>
          <p className="muted">
            Say which artist this credit is, and Hito groups its tracks under
            that artist.
          </p>
          <div className="identity-actions">
            <button
              ref={opener}
              id="choose-artist"
              type="button"
              className="text-action"
              onClick={open}
            >
              Choose the artist
            </button>
          </div>
        </>
      )}
      {choosing && (
        <Dialog
          wide
          title={`Which artist is ${artist.name}?`}
          description="Pick the artist this credit means. Nothing changes until you confirm."
          busy={busy}
          returnFocus={opener}
          onClose={closeChoosing}
        >
          {identity.step === "loading" && (
            <p role="status">Looking for artists you reviewed…</p>
          )}
          {showChoices && (
            <ArtistChoiceOptions
              credit={artist.name}
              suggestions={suggestionItems}
              groups={identity.step === "choosing" ? identity.groups : []}
              busy={busy}
              searchHref={
                searchRecording ? artistSearchHref(searchRecording) : null
              }
              onSuggestion={identity.choose}
              onGroup={identity.choose}
              onSearch={(href) => router.visit(href)}
              onNone={() =>
                suggestionItems.forEach(
                  (item) => void suggested.dismiss(item.group.id),
                )
              }
              onCancel={closeChoosing}
            />
          )}
          {(identity.step === "closed" || identity.step === "choosing") &&
            suggested.suggestions && (
              <ArtistMatchSuggestionList
                items={suggested.suggestions.items}
                dismissed={suggested.suggestions.dismissed_items}
                busy={suggested.busy}
                activeShown={false}
                onChoose={identity.choose}
                onDismiss={(group) => void suggested.dismiss(group)}
                onRestore={(group) => void suggested.restore(group)}
              />
            )}
          {identity.step === "choosing" && identity.groups.length === 0 && (
            <p className="dialog-empty">
              No artist you reviewed matches this credit yet.
            </p>
          )}
          {identity.step === "confirming" && identity.selected && (
            <div role="group" aria-label="Confirm artist identity">
              <p>
                Use {identity.selected.name.value} for every track you can
                access under the credit {artist.name}? A track keeps its choice
                when it already has a different reviewed artist, or when a
                different artist's choice needs review. Tracks never chosen,
                reset, or needing review for this artist are updated. You can
                reset any track later.
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
          {!showChoices && (
            <footer className="review-actions credit-dialog-actions">
              <Button disabled={busy} onClick={closeChoosing}>
                Close
              </Button>
            </footer>
          )}
        </Dialog>
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
