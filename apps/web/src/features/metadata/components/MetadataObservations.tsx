import type { Schema } from "../../../shared/schema";

import { useMetadataHistory } from "../hooks/useMetadataHistory";
import type { MetadataTarget } from "../model/metadata";
import RecordingCredits from "./RecordingCredits";
import Button from "../../../shared/ui/Button";
import { dateAndTime } from "../../../shared/format";

function observedAt(seconds: number) {
  return dateAndTime(new Date(seconds * 1000));
}

const tagLabels = {
  title: "Title",
  artist: "Artist",
  album: "Album",
  album_artist: "Album artist",
  disc: "Disc",
  track: "Track",
} as const;

function Observation({
  observation,
  selected,
}: {
  observation: Schema<"MetadataObservation">;
  selected: boolean;
}) {
  return (
    <li>
      <article>
        {observation.observed_at === null ? (
          <small className="track-when">Imported during upgrade</small>
        ) : (
          <time
            className="track-when"
            dateTime={new Date(observation.observed_at * 1000).toISOString()}
          >
            {observedAt(observation.observed_at)}
          </time>
        )}
        <span className="track-category">Read</span>
        <div className="track-observation">
          <h3>
            Metadata read from{" "}
            {observation.provider === "local"
              ? "local file"
              : observation.provider}
          </h3>
          <p>
            {observation.title} · {observation.artist} ·{" "}
            {observation.album ?? "Unknown album"}
            {selected && (
              <>
                {" · "}
                <strong className="metadata-selected">Current metadata</strong>
              </>
            )}
          </p>
          <RecordingCredits attribution={observation.attribution} />
          {observation.origin === "legacy_normalized" ? (
            <p className="metadata-legacy">
              Original tag details were not recorded. The next successful scan
              can inspect them.
            </p>
          ) : observation.origin === "provider_response" ? (
            <dl className="metadata-tag-list">
              <div>
                <dt>Provider recording</dt>
                <dd>{observation.provider_recording_id}</dd>
              </div>
              {observation.provider_revision && (
                <div>
                  <dt>Provider revision</dt>
                  <dd>{observation.provider_revision}</dd>
                </div>
              )}
            </dl>
          ) : (
            <details>
              <summary>Inspected tags</summary>
              <dl className="metadata-tag-list">
                {Object.entries(tagLabels).map(([key, label]) => {
                  const tag = observation.tags?.[key as keyof typeof tagLabels];
                  return (
                    <div key={key}>
                      <dt>{label}</dt>
                      <dd>
                        {tag ? (
                          <>
                            {tag.value}
                            <small>
                              {tag.location.kind === "format"
                                ? "File tags"
                                : `Audio stream ${tag.location.index + 1}`}
                            </small>
                          </>
                        ) : (
                          <span className="muted">No tag</span>
                        )}
                      </dd>
                    </div>
                  );
                })}
              </dl>
            </details>
          )}
        </div>
      </article>
    </li>
  );
}

export default function MetadataObservations({
  account,
  target,
  initial,
  selected,
}: {
  account: string;
  target: MetadataTarget;
  initial: Schema<"ObservationPage">;
  selected?: string;
}) {
  const history = useMetadataHistory(account, target, initial);
  return (
    <section
      className="rd-card metadata-history track-history"
      aria-labelledby="source-history-heading"
      aria-busy={history.pending}
    >
      <h2 id="source-history-heading" className="sr-only">
        Source observations
      </h2>
      {!history.isLatest && (
        <button
          className="text-link"
          disabled={history.pending}
          onClick={history.latest}
        >
          Latest observations
        </button>
      )}
      <ol className="metadata-observations">
        {history.page.items.map((observation) => (
          <Observation
            key={observation.id}
            observation={observation}
            selected={observation.id === selected}
          />
        ))}
      </ol>
      {history.page.items.length === 0 && (
        <p>No source observations have been recorded yet.</p>
      )}
      {history.error && (
        <p role="alert" className="form-error">
          {history.error}
        </p>
      )}
      {history.page.next_before !== null && (
        <Button
          className="track-older"
          disabled={history.pending}
          onClick={() => void history.older()}
        >
          {history.pending ? "Loading…" : "Older observations"}
        </Button>
      )}
    </section>
  );
}
