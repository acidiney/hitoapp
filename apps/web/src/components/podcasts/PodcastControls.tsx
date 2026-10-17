export default function PodcastControls({
  pending,
  waiting,
  checking,
  hasAttempt,
  error,
  statusError,
  refresh,
  retry,
  check,
}: {
  pending: boolean;
  waiting: boolean;
  checking: boolean;
  hasAttempt: boolean;
  error: string;
  statusError: string;
  refresh: () => Promise<void>;
  retry: () => Promise<void>;
  check: () => Promise<void>;
}) {
  return (
    <section aria-label="Podcast controls">
      <button
        className="button"
        disabled={pending || waiting || checking || hasAttempt}
        onClick={() => void refresh()}
      >
        {pending ? "Requesting refresh…" : "Refresh feed"}
      </button>
      {error && <p role="alert">{error}</p>}
      {hasAttempt && !pending && (
        <button className="button" onClick={() => void retry()}>
          Retry refresh request
        </button>
      )}
      {statusError && (
        <div>
          <p role="alert">{statusError}</p>
          <button
            className="button"
            disabled={pending || checking}
            onClick={() => void check()}
          >
            Check feed status
          </button>
        </div>
      )}
    </section>
  );
}
