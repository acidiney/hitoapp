export default function CatalogFailure({
  message,
  retryLabel,
  pending,
  onRetry,
  onRestart,
}: {
  message: string;
  retryLabel: string;
  pending: boolean;
  onRetry?: () => void;
  onRestart?: () => void;
}) {
  return (
    <div className="catalog-failure">
      <p role="alert" className="source-status" data-status="failed">
        <span className="source-status-dot" aria-hidden="true" />
        {message}
      </p>
      {onRetry && (
        <button
          type="button"
          className="text-action"
          aria-label={retryLabel}
          disabled={pending}
          onClick={onRetry}
        >
          Try again
        </button>
      )}
      {onRestart && (
        <button
          type="button"
          className="text-action"
          disabled={pending}
          onClick={onRestart}
        >
          Restart editions
        </button>
      )}
    </div>
  );
}
