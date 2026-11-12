import type { Schema } from "../../../shared/schema";
import { useContext, useEffect, useRef, useState } from "react";
import { AlbumMergeContext } from "./AlbumMergeContext";
import AlbumEditionTrackList from "../../artists/components/AlbumEditionTrackList";
import AlbumReleaseMatch from "./AlbumReleaseMatch";

import { useAlbumEditionChoice } from "../hooks/useAlbumEditionChoice";
import { useSavedAlbumEditionRead } from "../hooks/useSavedAlbumEditionRead";
import { useTaggedAlbumEdition } from "../hooks/useTaggedAlbumEdition";
import { albumEditionApi } from "../../artists/api/albumEditionApi";
import Button from "../../../shared/ui/Button";
import type { AlbumFacts } from "./AlbumEditionStep";

type Props = {
  account: string;
  release: string;
  album: AlbumFacts;
  anchor: Schema<"AlbumCatalogAnchor"> | null;
  saved: Schema<"AlbumEditionChoice">;
};
type View = "closed" | "saved" | "looking" | "tagged" | "browse";

export default function AlbumCompleteTrackList({
  account,
  release,
  album,
  anchor,
  saved,
}: Props) {
  const merge = useContext(AlbumMergeContext);
  const edition = useAlbumEditionChoice(account, release, saved);
  const [lookupTagged] = useState(
    anchor !== null && saved.state.status === "unset" && saved.revision === 0,
  );
  const [chosenView, setView] = useState<View>(
    lookupTagged ? "looking" : "closed",
  );
  const [tagged, setTagged] = useState<Schema<"SavedAlbumEdition"> | null>(
    null,
  );
  const lookup = useTaggedAlbumEdition(account, release);
  const state = edition.choice.state;
  const ready = state.status === "ready" ? state.edition : null;
  const linked = state.status !== "unset";
  const view: View = chosenView === "closed" && ready ? "saved" : chosenView;
  const browseAnchor: Schema<"AlbumCatalogAnchor"> | null =
    anchor ??
    (ready
      ? {
          recording_id: ready.recording_id,
          association_id: ready.association_id,
        }
      : null);

  const findTagged = useRef(lookup.find);
  findTagged.current = lookup.find;
  useEffect(() => {
    if (!lookupTagged) return;
    let active = true;
    void findTagged.current().then((found) => {
      if (!active || found === "stopped") return;
      if (found === "none") return setView("closed");
      setTagged(found);
      setView("tagged");
    });
    return () => {
      active = false;
    };
  }, [lookupTagged]);

  function stale() {
    edition.stale();
    setView(browseAnchor ? "browse" : "closed");
  }

  async function forget() {
    if (await edition.forget()) setView("closed");
  }

  return (
    <section aria-label="Complete track list" className="complete-track-list">
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
      {view === "closed" && browseAnchor && !ready && (
        <button
          type="button"
          className="text-action"
          onClick={() => setView("browse")}
        >
          Choose an edition
        </button>
      )}
      {view === "looking" && (
        <p role="status">Looking for the edition named in your file tags…</p>
      )}
      {view === "tagged" && tagged && (
        <>
          <p className="sr-only">
            This is the edition named by the release id in your file tags.
          </p>
          <SavedEdition
            key={`${tagged.release_group_id}/${tagged.edition_id}`}
            account={account}
            release={release}
            pending={false}
            recording={tagged.recording_id}
            canChange
            stale={() => setView("browse")}
            change={() => setView("browse")}
            read={(signal) =>
              albumEditionApi.tracks(account, release, tagged, signal)
            }
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
          forget={() => void forget()}
        />
      )}
      {view === "browse" && browseAnchor && (
        <AlbumReleaseMatch
          account={account}
          release={release}
          album={album}
          anchor={browseAnchor}
          dismiss={() => setView("closed")}
          forget={linked ? () => void forget() : undefined}
          busy={edition.pending}
          remembered={edition.remembered !== null}
          remember={(chosen) =>
            chosen ? void edition.choose(chosen) : edition.reading()
          }
        />
      )}
      {linked && !merge.merged && (
        <button
          type="button"
          className="text-action"
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
  recording,
  canChange,
  stale,
  change,
  forget,
  read: reader,
}: {
  account: string;
  release: string;
  pending: boolean;
  recording: string;
  canChange: boolean;
  stale: () => void;
  change: () => void;
  forget?: () => void;
  read?: (
    signal: AbortSignal,
  ) => Promise<Schema<"AlbumEditionTrackListAvailability">>;
}) {
  const read = useSavedAlbumEditionRead(account, release, stale, reader);
  return (
    <AlbumEditionTrackList
      account={account}
      source={{ release, recording }}
      comparison={read.comparison}
      pending={read.pending}
      error={read.error}
      retry={read.retry}
      edition={{
        change: canChange ? change : undefined,
        forget,
        busy: pending,
      }}
    />
  );
}
