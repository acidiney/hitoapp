import { useState } from "react";
import AlbumEditionTrackList from "../../artists/components/AlbumEditionTrackList";
import ArtistDiscography from "../../artists/components/ArtistDiscography";
import { useReviewedArtistProfile } from "../../artists/hooks/useReviewedArtistProfile";
import type {
  AlbumCatalogAnchor,
  AlbumEditionChoice,
  SavedAlbumEdition,
} from "../../../shared/types";
import { useAlbumEditionChoice } from "../hooks/useAlbumEditionChoice";
import { useSavedAlbumEditionRead } from "../hooks/useSavedAlbumEditionRead";

type Props = {
  account: string;
  release: string;
  anchor: AlbumCatalogAnchor | null;
  saved: AlbumEditionChoice;
};
type View = "closed" | "saved" | "browse";

export default function AlbumCompleteTrackList({
  account,
  release,
  anchor,
  saved,
}: Props) {
  const edition = useAlbumEditionChoice(account, release, saved);
  const [view, setView] = useState<View>("closed");
  const state = edition.choice.state;
  const ready = state.status === "ready" ? state.edition : null;
  const linked = state.status !== "unset";
  const browseAnchor: AlbumCatalogAnchor | null =
    anchor ??
    (ready
      ? {
          recording_id: ready.recording_id,
          association_id: ready.association_id,
        }
      : null);

  function open() {
    setView(ready ? "saved" : "closed");
    if (!ready && browseAnchor) setView("browse");
  }

  function stale() {
    edition.stale();
    setView(browseAnchor ? "browse" : "closed");
  }

  async function forget() {
    if (await edition.forget()) setView("closed");
  }

  return (
    <section aria-label="Complete track list">
      {state.status === "needs_choice" && (
        <p>
          Your saved edition can no longer be used. Choose an edition again.
          {browseAnchor === null &&
            " Choosing again needs a reviewed artist for this album."}
        </p>
      )}
      {edition.error && (
        <>
          <p role="alert">{edition.error}</p>
          <button
            type="button"
            className="button"
            disabled={edition.pending}
            onClick={() => void edition.retry()}
          >
            Try saving again
          </button>
        </>
      )}
      {view === "closed" && (browseAnchor || ready) && (
        <button type="button" className="button" onClick={open}>
          Show complete track list
        </button>
      )}
      {view === "saved" && ready && (
        <SavedEdition
          account={account}
          release={release}
          pending={edition.pending}
          canChange={browseAnchor !== null}
          stale={stale}
          change={() => setView("browse")}
        />
      )}
      {view === "browse" && browseAnchor && (
        <AlbumCatalogBrowser
          account={account}
          release={release}
          anchor={browseAnchor}
          remembered={edition.remembered !== null}
          remember={(chosen) =>
            chosen ? void edition.choose(chosen) : edition.reading()
          }
        />
      )}
      {linked && (
        <button
          type="button"
          className="button"
          disabled={edition.pending}
          onClick={() => void forget()}
        >
          Forget edition
        </button>
      )}
    </section>
  );
}

function SavedEdition({
  account,
  release,
  pending,
  canChange,
  stale,
  change,
}: {
  account: string;
  release: string;
  pending: boolean;
  canChange: boolean;
  stale: () => void;
  change: () => void;
}) {
  const read = useSavedAlbumEditionRead(account, release, stale);
  return (
    <>
      <h2>Complete track list</h2>
      <AlbumEditionTrackList
        account={account}
        comparison={read.comparison}
        pending={read.pending}
        error={read.error}
        retry={read.retry}
      />
      {canChange && (
        <button
          type="button"
          className="button"
          disabled={pending}
          onClick={change}
        >
          Change edition
        </button>
      )}
    </>
  );
}

function AlbumCatalogBrowser({
  account,
  release,
  anchor,
  remembered,
  remember,
}: {
  account: string;
  release: string;
  anchor: AlbumCatalogAnchor;
  remembered: boolean;
  remember: (edition: SavedAlbumEdition | null) => void;
}) {
  const artist = useReviewedArtistProfile(account, anchor);
  return (
    <div aria-busy={artist.pending}>
      <h2>Complete track list</h2>
      <p>
        Choose the release and edition to compare with this album. Choosing an
        edition remembers it for this album. Nothing else is saved until you
        play a catalog track.
      </p>
      {remembered && <p role="status">This edition is remembered.</p>}
      {artist.pending && <p role="status">Loading the reviewed artist…</p>}
      {artist.error && (
        <>
          <p role="alert">{artist.error}</p>
          <button
            type="button"
            className="button"
            disabled={artist.pending}
            onClick={() => void artist.retry()}
          >
            Try again
          </button>
        </>
      )}
      {artist.ready && (
        <ArtistDiscography
          account={account}
          recording={anchor.recording_id}
          association={artist.ready.association}
          profile={artist.ready.profile}
          browseOnMount
          release={release}
          onAlbumEdition={remember}
        />
      )}
    </div>
  );
}
