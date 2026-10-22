import type {
  PreparationReason,
  SourcePreparationPhase,
  SourcePreparationStatus,
} from "../shared/types";

const phases: Record<SourcePreparationPhase, string> = {
  queued: "Audio preparation is queued.",
  submitting: "Requesting audio preparation…",
  preparing: "Preparing audio…",
  ready: "Audio preparation finished.",
  blocked: "Audio preparation is blocked.",
  failed: "Audio preparation failed.",
  submission_unknown: "The music service request could not be confirmed.",
  cancelled: "Audio preparation stopped because access changed.",
};
const reasons: Record<PreparationReason, string> = {
  authorization_required: "Reconnect your music service to continue.",
  quota_reached: "Your music service quota has been reached.",
  rate_limited: "Your music service is limiting requests.",
  provider_restricted: "Your music service restricts this audio.",
  source_unavailable: "This audio is unavailable from your music service.",
  source_changed: "This audio source changed. Check availability again.",
  unsupported: "Your music service does not support this preparation.",
  provider_unavailable: "Your music service is temporarily unavailable.",
  invalid_response: "Your music service returned an invalid response.",
};

type Props = {
  status: SourcePreparationStatus | null;
  checking: boolean;
  pending: boolean;
  retrying: boolean;
  canStart: boolean;
  halted: boolean;
  error: string;
  statusError: string;
  start(): Promise<void>;
  retry(): Promise<void>;
  check(): Promise<void>;
};

export default function SourcePreparation({
  status,
  checking,
  pending,
  retrying,
  canStart,
  halted,
  error,
  statusError,
  start,
  retry,
  check,
}: Props) {
  return (
    <div>
      <p role="status">
        {pending
          ? "Requesting audio preparation…"
          : status
            ? phases[status.phase]
            : checking
              ? "Checking preparation…"
              : null}
      </p>
      {status?.phase === "preparing" && (
        <div>
          <progress
            aria-label="Audio preparation progress"
            max={10_000}
            value={status.progress_basis_points ?? undefined}
          />
          {status.progress_basis_points != null && (
            <span> {Math.floor(status.progress_basis_points / 100)}%</span>
          )}
          {status.eta_seconds != null && (
            <p>Estimated time remaining: {status.eta_seconds} seconds.</p>
          )}
        </div>
      )}
      {status?.reason && <p>{reasons[status.reason]}</p>}
      {error && (
        <p role="alert" className="form-error">
          {error}
        </p>
      )}
      {statusError && (
        <p role="alert" className="form-error">
          {statusError}
        </p>
      )}
      {(retrying || canStart) && (
        <button
          className="button"
          disabled={pending || checking}
          onClick={() => void (retrying ? retry() : start())}
        >
          {retrying ? "Retry preparation request" : "Prepare audio"}
        </button>
      )}
      {statusError && !halted && status?.phase !== "ready" && (
        <button
          className="button"
          disabled={checking || pending || retrying}
          onClick={() => void check()}
        >
          Check preparation
        </button>
      )}
    </div>
  );
}
