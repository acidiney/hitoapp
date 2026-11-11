import type { Schema } from "../../../shared/schema";
import { useEffect, useRef, useState } from "react";
import AlbumEditionTrackList from "../../artists/components/AlbumEditionTrackList";
import { useAlbumEditionTrackList } from "../../artists/hooks/useAlbumEditionTrackList";
import {
  useArtistEditions,
  type ArtistEditionContext,
} from "../../artists/hooks/useArtistEditions";
import { releaseYear } from "../model/releaseMatch";
import EditionChoicePanel from "./EditionChoicePanel";
import MatchPanel from "./MatchPanel";

export type AlbumFacts = {
  title: string;
  year: number | null;
  trackCount: number;
};

type Props = {
  context: ArtistEditionContext & { release: string };
  album: AlbumFacts;
  suggested: boolean;
  automatic: boolean;
  remembered: boolean;
  remember: (edition: Schema<"SavedAlbumEdition"> | null) => void;
  different: () => void;
  dismiss: () => void;
};

export default function AlbumEditionStep({
  context,
  album,
  suggested,
  automatic,
  remembered,
  remember,
  different,
  dismiss,
}: Props) {
  const { group, release } = context;
  const workflow = useArtistEditions(context, true);
  const comparison = useAlbumEditionTrackList(
    context,
    release,
    workflow.chosen,
  );
  const rememberLatest = useRef(remember);
  rememberLatest.current = remember;
  const chooseLatest = useRef(workflow.choose);
  chooseLatest.current = workflow.choose;
  const [selected, setSelected] = useState("");
  const [changing, setChanging] = useState(false);
  const read = comparison.comparison;
  const year = releaseYear(group);
  const settled =
    workflow.pages > 0 &&
    !workflow.pending &&
    !workflow.error &&
    workflow.continuation === null;
  useEffect(() => {
    rememberLatest.current(null);
  }, [workflow.chosen]);
  useEffect(() => {
    if (read)
      rememberLatest.current({
        recording_id: context.recording,
        association_id: read.association_id,
        release_group_id: read.edition.release_group_id,
        edition_id: read.edition.id,
      });
  }, [read, context.recording]);
  useEffect(() => {
    if (
      automatic &&
      settled &&
      !workflow.chosen &&
      workflow.editions.length === 1
    )
      chooseLatest.current(workflow.editions[0].id);
  }, [automatic, settled, workflow.chosen, workflow.editions]);

  const retry = () =>
    workflow.restartRequired || (workflow.error && workflow.pages > 0)
      ? workflow.restart()
      : workflow.retry();

  if (workflow.chosen && !changing)
    return (
      <div className="match-lined-up">
        <p className="match-lined-up-line">
          <span>
            Lined up with {group.title}
            {year ? ` (${year})` : ""}
          </span>
          <button
            type="button"
            className="text-action"
            aria-label="Change edition"
            disabled={comparison.pending}
            onClick={() => setChanging(true)}
          >
            Change
          </button>
        </p>
        {remembered && (
          <p role="status" className="sr-only">
            This edition is remembered.
          </p>
        )}
        <AlbumEditionTrackList
          account={context.account}
          source={{ release, recording: context.recording }}
          comparison={comparison.comparison}
          pending={comparison.pending}
          error={comparison.error}
          retry={comparison.retry}
        />
      </div>
    );

  if (workflow.error)
    return (
      <MatchPanel title="Showing only your tracks">
        <p role="alert" className="match-alert">
          {workflow.error} Your {album.trackCount}{" "}
          {album.trackCount === 1 ? "track is" : "tracks are"} listed and play
          as usual. The full track list needs an edition.
        </p>
        <div className="match-actions">
          <button
            type="button"
            className="text-action"
            disabled={workflow.pending}
            onClick={() => void retry()}
          >
            Try again
          </button>
          <button type="button" className="text-action" onClick={dismiss}>
            Just show my tracks
          </button>
        </div>
      </MatchPanel>
    );

  if (settled && workflow.editions.length === 0)
    return (
      <MatchPanel title="No editions listed for this album">
        <p className="match-alert">
          The catalog lists no edition of this release. Your {album.trackCount}{" "}
          {album.trackCount === 1 ? "track is" : "tracks are"} listed and play
          as usual.
        </p>
        <div className="match-actions">
          <button type="button" className="text-action" onClick={different}>
            It's a different release
          </button>
          <button type="button" className="text-action" onClick={dismiss}>
            Just show my tracks
          </button>
        </div>
      </MatchPanel>
    );

  const title =
    suggested && !changing
      ? `Is this ${group.title}${year ? ` from ${year}` : ""}?`
      : group.title;
  const current =
    selected || workflow.chosen?.id || workflow.editions[0]?.id || "";
  return (
    <EditionChoicePanel
      title={title}
      group={group}
      suggested={suggested}
      editions={workflow.editions}
      selected={current}
      pending={workflow.pending}
      select={setSelected}
      use={() => {
        workflow.choose(current);
        setChanging(false);
      }}
      different={different}
      dismiss={dismiss}
      more={
        workflow.continuation && !workflow.pending
          ? () => void workflow.more()
          : null
      }
    />
  );
}
