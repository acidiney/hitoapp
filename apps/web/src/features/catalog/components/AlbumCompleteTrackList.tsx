import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import AlbumEditionTrackList from "../../artists/components/AlbumEditionTrackList";
import ArtistDiscography from "../../artists/components/ArtistDiscography";
import { useReviewedArtistProfile } from "../../artists/hooks/useReviewedArtistProfile";

import { useAlbumEditionChoice } from "../hooks/useAlbumEditionChoice";
import { useSavedAlbumEditionRead } from "../hooks/useSavedAlbumEditionRead";
import { useTaggedAlbumEdition } from "../hooks/useTaggedAlbumEdition";
import { albumEditionApi } from "../../artists/api/albumEditionApi";
import Button from "../../../shared/ui/Button";

type Props = {
  account: string;
  release: string;
  anchor: Schema<"AlbumCatalogAnchor"> | null;
  saved: Schema<"AlbumEditionChoice">;
};
type View = "closed" | "saved" | "looking" | "tagged" | "browse";

export default function AlbumCompleteTrackList({
  account,
  release,
  anchor,
  saved,
}: Props) {
  const edition = useAlbumEditionChoice(account, release, saved);
  const [view, setView] = useState<View>("closed");
  const [tagged, setTagged] = useState<Schema<"SavedAlbumEdition"> | null>(
    null,
  );
  const lookup = useTaggedAlbumEdition(account, release);
  const state = edition.choice.state;
  const ready = state.status === "ready" ? state.edition : null;
  const linked = state.status !== "unset";
  const browseAnchor: Schema<"AlbumCatalogAnchor"> | null =
    anchor ??
    (ready
      ? {
          recording_id: ready.recording_id,
          association_id: ready.association_id,
        }
      : null);

  async function open() {
    if (ready) return setView("saved");
    if (!browseAnchor) return;
    if (state.status === "unset" && edition.choice.revision === 0) {
      setView("looking");
      const found = await lookup.find();
      if (found === "stopped") return;
      if (found !== "none") {
        setTagged(found);
        return setView("tagged");
      }
    }
    setView("browse");
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
          <Button
            type="button"
            disabled={edition.pending}
            onClick={() => void edition.retry()}
          >
            Try saving again
          </Button>
        </>
      )}
      {view === "closed" && (browseAnchor || ready) && (
        <Button type="button" onClick={() => void open()}>
          Show complete track list
        </Button>
      )}
      {view === "looking" && (
        <p role="status">Looking for the edition named in your file tags…</p>
      )}
      {view === "tagged" && tagged && (
        <>
          <p>This is the edition named by the release id in your file tags.</p>
          <SavedEdition
            key={`${tagged.release_group_id}/${tagged.edition_id}`}
            account={account}
            release={release}
            pending={false}
            recording={tagged.recording_id}
            canChange
            stale={() => setView("browse")}
            change={() => setView("browse")}
            read={(signal) => albumEditionApi.tracks(account, release, tagged, signal)}
          />
        </>
      )}
      {view === "saved" && ready && (
        <SavedEdition
          account={account}
          release={release}
          pending={edition.pending}
          recording={ready!.recording_id}
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
        <Button
          type="button"
          disabled={edition.pending}
          onClick={() => void forget()}
        >
          Forget edition
        </Button>
      )}
    </section>
  );
}

function SavedEdition({
  account,
  release,
  pending,
  recording,
  canChange,
  stale,
  change,
  read: reader,
}: {
  account: string;
  release: string;
  pending: boolean;
  recording: string;
  canChange: boolean;
  stale: () => void;
  change: () => void;
  read?: (signal: AbortSignal) => Promise<Schema<"AlbumEditionTrackListAvailability">>;
}) {
  const read = useSavedAlbumEditionRead(account, release, stale, reader);
  return (
    <>
      <h2>Complete track list</h2>
      <AlbumEditionTrackList
        account={account}
        source={{ release, recording }}
        comparison={read.comparison}
        pending={read.pending}
        error={read.error}
        retry={read.retry}
      />
      {canChange && (
        <Button type="button" disabled={pending} onClick={change}>
          Change edition
        </Button>
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
  anchor: Schema<"AlbumCatalogAnchor">;
  remembered: boolean;
  remember: (edition: Schema<"SavedAlbumEdition"> | null) => void;
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
          <Button
            type="button"
            disabled={artist.pending}
            onClick={() => void artist.retry()}
          >
            Try again
          </Button>
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
