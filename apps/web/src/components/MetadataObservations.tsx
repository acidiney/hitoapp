import type { MetadataObservation, ObservationPage } from "../types";
import { useMetadataHistory } from "../hooks/useMetadataHistory";
import type { MetadataTarget } from "../metadata";

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
  observation: MetadataObservation;
  selected: boolean;
}) {
  return (
    <li>
      <article>
        <header>
          <span>
            {observation.provider === "local"
              ? "Local file"
              : observation.provider}
            {selected && (
              <strong className="metadata-selected">Current source</strong>
            )}
          </span>
          {observation.observed_at === null ? (
            <small>Imported during upgrade</small>
          ) : (
            <time
              dateTime={new Date(observation.observed_at * 1000).toISOString()}
            >
              {new Date(observation.observed_at * 1000).toLocaleString()}
            </time>
          )}
        </header>
        <h3>{observation.title}</h3>
        <p>
          {observation.artist} · {observation.album ?? "Unknown album"}
        </p>
        {observation.origin === "legacy_normalized" ? (
          <p className="metadata-legacy">
            Original tag details were not recorded. The next successful scan can
            inspect them.
          </p>
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
  initial: ObservationPage;
  selected?: string;
}) {
  const history = useMetadataHistory(account, target, initial);
  return (
    <section
      className="metadata-history"
      aria-labelledby="source-history-heading"
      aria-busy={history.pending}
    >
      <h2 id="source-history-heading">Source observations</h2>
      <p>
        Metadata read from your library. Personal edits leave these observations
        intact.
      </p>
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
        <button
          className="button"
          disabled={history.pending}
          onClick={() => void history.older()}
        >
          {history.pending ? "Loading…" : "Older observations"}
        </button>
      )}
    </section>
  );
}
