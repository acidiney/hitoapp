import type { Schema } from "../../../shared/schema";
import { useState } from "react";
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
      {view === "closed" && (browseAnchor || ready) && (
        <button
          type="button"
          className="text-action"
          onClick={() => void open()}
        >
          Show complete track list
        </button>
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
        />
      )}
      {view === "browse" && browseAnchor && (
        <AlbumReleaseMatch
          account={account}
          release={release}
          album={album}
          anchor={browseAnchor}
          dismiss={() => setView("closed")}
          remembered={edition.remembered !== null}
          remember={(chosen) =>
            chosen ? void edition.choose(chosen) : edition.reading()
          }
        />
      )}
      {linked && (
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
  read: reader,
}: {
  account: string;
  release: string;
  pending: boolean;
  recording: string;
  canChange: boolean;
  stale: () => void;
  change: () => void;
  read?: (
    signal: AbortSignal,
  ) => Promise<Schema<"AlbumEditionTrackListAvailability">>;
}) {
  const read = useSavedAlbumEditionRead(account, release, stale, reader);
  return (
    <>
      {read.comparison && (
        <p className="match-lined-up-line">
          <span>Lined up with {read.comparison.edition.title}</span>
          {canChange && (
            <button
              type="button"
              className="text-action"
              aria-label="Change edition"
              disabled={pending}
              onClick={change}
            >
              Change
            </button>
          )}
        </p>
      )}
      <AlbumEditionTrackList
        account={account}
        source={{ release, recording }}
        comparison={read.comparison}
        pending={read.pending}
        error={read.error}
        retry={read.retry}
      />
    </>
  );
}
