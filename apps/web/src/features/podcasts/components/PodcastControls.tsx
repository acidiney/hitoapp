import Button from "../../../shared/ui/Button";

type Controls = {
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
};

export function PodcastRefreshButton({
  pending,
  waiting,
  checking,
  hasAttempt,
  attemptKind,
  leaving,
  refresh,
}: Controls) {
  return (
    <button
      type="button"
      className="button tint-secondary"
      disabled={pending || waiting || checking || hasAttempt || leaving}
      onClick={() => void refresh()}
    >
      {pending && attemptKind === "refresh"
        ? "Requesting refresh…"
        : "Refresh feed"}
    </button>
  );
}

export function PodcastNotices({
  pending,
  checking,
  hasAttempt,
  attemptKind,
  leaving,
  error,
  statusError,
  retry,
  check,
}: Controls) {
  if (!leaving && !error && !statusError && !(hasAttempt && !pending))
    return null;
  return (
    <section className="media-notices" aria-label="Podcast controls">
      {leaving && <p role="status">Unsubscribed. Opening all podcasts…</p>}
      {error && <p role="alert">{error}</p>}
      {hasAttempt && !pending && !leaving && (
        <Button onClick={() => void retry()}>
          {attemptKind === "unsubscribe"
            ? "Retry unsubscribe request"
            : "Retry refresh request"}
        </Button>
      )}
      {statusError && (
        <div>
          <p role="alert">{statusError}</p>
          <Button
            disabled={pending || checking || leaving}
            onClick={() => void check()}
          >
            Check feed status
          </Button>
        </div>
      )}
    </section>
  );
}

export function PodcastOptions({
  pending,
  hasAttempt,
  attemptKind,
  leaving,
  unsubscribe,
}: Controls) {
  return (
    <aside className="media-side" aria-labelledby="podcast-options">
      <h2 id="podcast-options">For this podcast</h2>
      <div className="media-card-panel">
        <p className="media-side-note">
          Unsubscribing stops feed updates and podcast audio. Episode details
          and your listening data remain.
        </p>
      </div>
      <Button
        variant="destructive"
        disabled={pending || hasAttempt || leaving}
        onClick={() => void unsubscribe()}
      >
        {pending && attemptKind === "unsubscribe"
          ? "Unsubscribing…"
          : "Unsubscribe"}
      </Button>
    </aside>
  );
}
