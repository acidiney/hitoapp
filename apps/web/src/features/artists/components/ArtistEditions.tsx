import { useEffect, useState } from "react";
import {
  useArtistEditions,
  type ArtistEditionContext,
} from "../hooks/useArtistEditions";
import { useAlbumEditionTrackList } from "../hooks/useAlbumEditionTrackList";
import { useArtistEditionTrackList } from "../hooks/useArtistEditionTrackList";
import AlbumEditionTrackList from "./AlbumEditionTrackList";
import ArtistEditionTrackList from "./ArtistEditionTrackList";
import Button from "../../../shared/ui/Button";

export default function ArtistEditions(context: ArtistEditionContext) {
  const [opened, setOpened] = useState(false);
  if (!opened)
    return (
      <Button type="button" onClick={() => setOpened(true)}>
        Browse editions
      </Button>
    );
  return <ArtistEditionBrowser {...context} />;
}

function ArtistEditionBrowser(context: ArtistEditionContext) {
  const workflow = useArtistEditions(context, true);
  const tracks = useArtistEditionTrackList(
    context,
    context.release === undefined ? workflow.chosen : null,
  );
  const comparison = useAlbumEditionTrackList(
    context,
    context.release ?? "",
    context.release === undefined ? null : workflow.chosen,
  );
  const read = comparison.comparison;
  useEffect(() => {
    context.onAlbumEdition?.(null);
  }, [workflow.chosen]);
  useEffect(() => {
    if (read)
      context.onAlbumEdition?.({
        recording_id: context.recording,
        association_id: read.association_id,
        release_group_id: read.edition.release_group_id,
        edition_id: read.edition.id,
      });
  }, [read]);
  if (!workflow.opened)
    return (
      <Button type="button" onClick={() => void workflow.browse()}>
        Browse editions
      </Button>
    );
  return (
    <section aria-label="Editions" aria-busy={workflow.pending}>
      <h5>Editions</h5>
      {workflow.pending && (
        <p role="status">
          {workflow.pages > 0 ? "Loading more editions…" : "Loading editions…"}
        </p>
      )}
      {workflow.error && <p role="alert">{workflow.error}</p>}
      <ul aria-label="Available editions">
        {workflow.editions.map((edition) => (
          <li key={edition.id}>
            <h5>{edition.title}</h5>
            <p>{edition.disambiguation ?? "Edition details not provided"}</p>
            <p>
              {edition.artist_credit?.length
                ? edition.artist_credit
                    .map((member) => member.credited_name + member.join_phrase)
                    .join("")
                : "Artist credit not provided"}
            </p>
            <p>{edition.release_date ?? "Release date not provided"}</p>
            <p>{edition.country ?? "Country not provided"}</p>
            <Button
              type="button"
              pressed={workflow.chosen?.id === edition.id}
              disabled={workflow.pending}
              onClick={() => workflow.choose(edition.id)}
            >
              Choose edition
            </Button>
          </li>
        ))}
      </ul>
      {workflow.pages > 0 &&
        workflow.editions.length === 0 &&
        !workflow.pending &&
        !workflow.error && (
          <p>No editions were returned for this release group.</p>
        )}
      {workflow.limited && <p>Showing the first 50 pages of editions.</p>}
      {workflow.error && !workflow.restartRequired && (
        <Button
          type="button"
          disabled={workflow.pending}
          onClick={() => void workflow.retry()}
        >
          Try editions again
        </Button>
      )}
      {(workflow.restartRequired || (workflow.error && workflow.pages > 0)) && (
        <Button
          type="button"
          disabled={workflow.pending}
          onClick={() => void workflow.restart()}
        >
          Restart editions
        </Button>
      )}
      {!workflow.error && workflow.continuation && (
        <Button
          type="button"
          disabled={workflow.pending}
          onClick={() => void workflow.more()}
        >
          Load more editions
        </Button>
      )}
      {workflow.chosen &&
        (context.release === undefined ? (
          <ArtistEditionTrackList
            edition={tracks.edition}
            pending={tracks.pending}
            error={tracks.error}
            retry={tracks.retry}
          />
        ) : (
          <AlbumEditionTrackList
            account={context.account}
            source={{ release: context.release, recording: context.recording }}
            comparison={comparison.comparison}
            pending={comparison.pending}
            error={comparison.error}
            retry={comparison.retry}
          />
        ))}
    </section>
  );
}
