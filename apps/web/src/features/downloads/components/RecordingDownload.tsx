import type { DownloadStatus } from "../../../shared/types";

type Props = {
  download: DownloadStatus | null;
  checking: boolean;
  pending: "create" | "remove" | null;
  retryKind: "create" | "remove" | null;
  error: string;
  statusError: string;
  local: boolean;
  eligibilityPending?: boolean;
  start(): Promise<void>;
  remove(): Promise<void>;
  retry(): Promise<void>;
  check(): Promise<void>;
};

export default function RecordingDownload({
  download,
  checking,
  pending,
  retryKind,
  error,
  statusError,
  local,
  eligibilityPending = false,
  start,
  remove,
  retry,
  check,
}: Props) {
  const verified = !eligibilityPending && !checking && !statusError;
  const previousSource = download?.matches_source_choice === false;
  const transferring =
    download !== null &&
    ["queued", "running", "verifying"].includes(download.status);
  const label =
    pending === "create"
      ? previousSource
        ? "Download selected source"
        : "Download"
      : pending === "remove"
        ? "Remove download"
        : retryKind === "create"
          ? "Retry download request"
          : retryKind === "remove"
            ? "Retry removal"
            : download
              ? "Remove download"
              : "Download";
  const action = retryKind ? retry : download ? remove : start;
  return (
    <section aria-labelledby="recording-download-heading">
      <h2 id="recording-download-heading">Download</h2>
      <p>
        Save a copy on this {local ? "device" : "server"} to listen when its
        music service is unavailable.
      </p>
      <p role="status">
        {checking || eligibilityPending
          ? "Checking saved copy…"
          : statusError
            ? "Saved copy status is unavailable."
            : download?.status === "ready"
              ? previousSource
                ? `A copy from a previous source is saved on this ${local ? "device" : "server"}.`
                : local
                  ? "Saved on this device"
                  : "Saved on this server"
              : download?.status === "queued"
                ? "Download queued."
                : download?.status === "running"
                  ? "Downloading audio…"
                  : download?.status === "verifying"
                    ? "Verifying audio…"
                    : download?.status === "failed"
                      ? "Download failed. Remove it before requesting another copy."
                      : null}
      </p>
      {verified && previousSource && download?.status !== "ready" && (
        <p role="status">This download belongs to a previous source.</p>
      )}
      {transferring && (
        <progress
          aria-label="Download progress"
          max={
            download.total_bytes && download.total_bytes > 0
              ? download.total_bytes
              : undefined
          }
          value={
            download.total_bytes && download.total_bytes > 0
              ? download.bytes_received
              : undefined
          }
        />
      )}
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      {statusError && (
        <div>
          <p className="form-error" role="alert">
            {statusError}
          </p>
          <button
            className="button"
            disabled={checking || pending !== null}
            onClick={() => void check()}
          >
            Check download
          </button>
        </div>
      )}
      {previousSource && !retryKind && (
        <button
          className="button"
          disabled={!verified || pending !== null}
          onClick={() => void start()}
        >
          Download selected source
        </button>
      )}
      <button
        className="button"
        disabled={pending !== null || (!retryKind && !verified)}
        onClick={() => void action()}
      >
        {label}
      </button>
    </section>
  );
}
