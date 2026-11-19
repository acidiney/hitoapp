import { Link } from "@inertiajs/react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { useArtistCreditIdentity } from "../hooks/useArtistCreditIdentity";
import { artistIdentityApi } from "../api/artistIdentityApi";
import ArtistChoiceOptions from "./ArtistChoiceOptions";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";

function plural(count: number, one: string, many: string) {
  return `${count} ${count === 1 ? one : many}`;
}

export default function MemberArtistIdentity({
  account,
  artist,
}: {
  account: string;
  artist: { id: string; name: string };
}) {
  const identity = useArtistCreditIdentity(account, artist, "members");
  const [read, setRead] = useState<Schema<"MemberArtistIdentity"> | null>(null);
  const [choosing, setChoosing] = useState(false);
  const opener = useRef<HTMLButtonElement>(null);
  const reload = useCallback(
    (signal: AbortSignal) =>
      artistIdentityApi
        .memberIdentity(account, artist.id, signal)
        .then((value) => {
          if (!signal.aborted) setRead(value);
        })
        .catch(() => {}),
    [account, artist.id],
  );
  useEffect(() => {
    const controller = new AbortController();
    void reload(controller.signal);
    return () => controller.abort();
  }, [reload, identity.totals]);
  useEffect(() => {
    if (identity.step === "closed" && identity.totals) setChoosing(false);
  }, [identity.step, identity.totals]);
  const busy = identity.step === "loading" || identity.step === "applying";
  const close = () => {
    setChoosing(false);
    identity.cancel();
  };
  const open = () => {
    setChoosing(true);
    identity.find();
  };
  const chosen = read?.chosen ?? [];
  return (
    <section
      className="detail-section"
      aria-label="Artist identity for this name"
      aria-busy={busy}
    >
      <h2>Artist identity</h2>
      {chosen.length > 0 ? (
        chosen.map((item) => (
          <p key={item.group_id} aria-live="polite">
            {chosen.length > 1
              ? `${plural(item.tracks, "appearance", "appearances")} use a chosen artist`
              : "Artist chosen"}{" "}
            ·{" "}
            <Link
              className="text-link"
              href={`/artist-groups/${encodeURIComponent(item.group_id)}`}
            >
              Open artist
            </Link>
          </p>
        ))
      ) : (
        <p className="identity-row" aria-live="polite">
          {read && <span>Not identified yet</span>}
          {read && <span aria-hidden="true">·</span>}
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
      {chosen.length > 0 && (read?.undecided_tracks ?? 0) > 0 && (
        <p className="muted">
          {plural(read!.undecided_tracks, "appearance", "appearances")} not
          matched yet.{" "}
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
      {choosing && (
        <Dialog
          wide
          title={`Which artist is ${artist.name}?`}
          description="Pick the artist this name means wherever it appears in a credit. Nothing changes until you confirm."
          busy={busy}
          returnFocus={opener}
          onClose={close}
        >
          {identity.step === "loading" && (
            <p role="status">Looking for artists you reviewed…</p>
          )}
          {identity.step === "choosing" && (
            <ArtistChoiceOptions
              credit={artist.name}
              suggestions={[]}
              groups={identity.groups}
              busy={busy}
              searchHref={null}
              onSuggestion={identity.choose}
              onGroup={identity.choose}
              onSearch={() => {}}
              onNone={() => {}}
              onCancel={close}
            />
          )}
          {identity.step === "choosing" && identity.groups.length === 0 && (
            <p className="dialog-empty">
              No artist you reviewed matches this name yet. Review it on one of
              the tracks it appears on first.
            </p>
          )}
          {identity.step === "confirming" && identity.selected && (
            <div
              role="group"
              aria-label="Confirm artist identity"
              className="identity-confirm"
            >
              <p>
                Use {identity.selected.name.value} wherever {artist.name}{" "}
                appears in a credit?
              </p>
              <p className="muted">
                Appearances with a different reviewed artist keep their choice.
                You can reset any track later.
              </p>
              <footer className="review-actions credit-dialog-actions">
                <Button type="button" onClick={identity.cancel}>
                  Cancel
                </Button>
                <Button
                  type="button"
                  variant="primary"
                  onClick={() => identity.apply(identity.selected!)}
                >
                  Confirm
                </Button>
              </footer>
            </div>
          )}
          {identity.step === "applying" && <p>Applying the artist identity…</p>}
          {identity.error && <p role="alert">{identity.error}</p>}
          {identity.retrying && (
            <Button type="button" disabled={busy} onClick={identity.retry}>
              Retry the same artist identity request
            </Button>
          )}
        </Dialog>
      )}
      {identity.totals && identity.selected && (
        <div role="status">
          <p>
            {plural(
              identity.totals.applied,
              "appearance now uses",
              "appearances now use",
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
        </div>
      )}
    </section>
  );
}
