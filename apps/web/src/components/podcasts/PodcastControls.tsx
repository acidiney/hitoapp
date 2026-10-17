export default function PodcastControls({
  pending,
  waiting,
  checking,
  hasAttempt,
  attemptKind,
  leaving,
  error,
  statusError,
  refresh,
  unsubscribe,
  retry,
  check,
}: {
  pending: boolean;
  waiting: boolean;
  checking: boolean;
  hasAttempt: boolean;
  attemptKind: "refresh" | "unsubscribe" | null;
  leaving: boolean;
  error: string;
  statusError: string;
  refresh: () => Promise<void>;
  unsubscribe: () => Promise<void>;
  retry: () => Promise<void>;
  check: () => Promise<void>;
}) {
  return (
    <section aria-label="Podcast controls">
      <button
        className="button"
        disabled={pending || waiting || checking || hasAttempt || leaving}
        onClick={() => void refresh()}
      >
        {pending && attemptKind === "refresh"
          ? "Requesting refresh…"
          : "Refresh feed"}
      </button>
      <button
        className="button danger"
        disabled={pending || hasAttempt || leaving}
        onClick={() => void unsubscribe()}
      >
        {pending && attemptKind === "unsubscribe"
          ? "Unsubscribing…"
          : "Unsubscribe"}
      </button>
      <p className="muted">
        Unsubscribing stops feed updates and podcast audio. Episode details and
        your listening data remain.
      </p>
      {leaving && <p role="status">Unsubscribed. Opening all podcasts…</p>}
      {error && <p role="alert">{error}</p>}
      {hasAttempt && !pending && !leaving && (
        <button className="button" onClick={() => void retry()}>
          {attemptKind === "unsubscribe"
            ? "Retry unsubscribe request"
            : "Retry refresh request"}
        </button>
      )}
      {statusError && (
        <div>
          <p role="alert">{statusError}</p>
          <button
            className="button"
            disabled={pending || checking || leaving}
            onClick={() => void check()}
          >
            Check feed status
          </button>
        </div>
      )}
    </section>
  );
}
