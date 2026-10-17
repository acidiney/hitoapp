import type { PlaylistDownloadStatus } from "../../types";
import { usePlaylistDownload } from "../../hooks/usePlaylistDownload";

type ViewProps = {
  download: PlaylistDownloadStatus | null;
  checking: boolean;
  pending: boolean;
  requested: boolean;
  hasRetainedFailures: boolean;
  retryKind: "create" | "remove" | null;
  error: string;
  statusError: string;
  local: boolean;
  start(): Promise<void>;
  remove(): Promise<void>;
  retryFailed(): Promise<void>;
  retry(): Promise<void>;
  check(): Promise<void>;
};

export default function PlaylistDownload({
  account,
  playlist,
  local,
}: {
  account: string;
  playlist: string;
  local: boolean;
}) {
  const workflow = usePlaylistDownload(account, playlist);
  return <PlaylistDownloadView {...workflow} local={local} />;
}

function PlaylistDownloadView({
  download,
  checking,
  pending,
  requested,
  hasRetainedFailures,
  retryKind,
  error,
  statusError,
  local,
  start,
  remove,
  retryFailed,
  retry,
  check,
}: ViewProps) {
  const entries = download?.entries.items ?? [];
  const count = (state: string) =>
    entries.filter((entry) => entry.status === state).length;
  const ready = count("ready");
  const waiting = count("waiting_for_source");
  const failed = count("failed");
  const unavailable = count("unavailable");
  const partial = download?.entries.next_cursor != null;
  const retrying = retryKind !== null && !pending;
  const label = retrying
    ? retryKind === "remove"
      ? "Retry removal"
      : "Retry download request"
    : requested
      ? "Remove download"
      : "Download";
  const action = retryKind ? retry : requested ? remove : start;
  return (
    <section aria-labelledby="playlist-download-heading">
      <h2 id="playlist-download-heading">Playlist download</h2>
      <p>Copies are saved on this {local ? "device" : "server"}.</p>
      {!local && (
        <p>This browser still needs a connection to this server to listen.</p>
      )}
      <p role="status">
        {download
          ? entries.length === 0
            ? "No playlist entries to download."
            : `${ready} of ${entries.length} ${partial ? "checked entries" : "entries"} saved.`
          : checking
            ? "Checking saved copies…"
            : requested
              ? "Download requested."
              : null}
      </p>
      {download && entries.length > 0 && (
        <progress
          aria-label="Playlist download progress"
          max={entries.length}
          value={ready}
        />
      )}
      {partial && (
        <p>
          Only the first {entries.length} entries are checked here. More entries
          have not been checked.
        </p>
      )}
      {download && download.reconciled_revision < download.desired_revision && (
        <p>Updating playlist download membership…</p>
      )}
      {waiting > 0 && (
        <p>
          {waiting} {waiting === 1 ? "entry" : "entries"} waiting for a source.
        </p>
      )}
      {failed > 0 && (
        <p>
          {failed} {failed === 1 ? "entry could" : "entries could"} not be
          downloaded.
        </p>
      )}
      {unavailable > 0 && (
        <p>
          {unavailable} {unavailable === 1 ? "entry is" : "entries are"}{" "}
          unavailable.
        </p>
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
            disabled={checking || pending || retryKind !== null}
            onClick={() => void check()}
          >
            Check download
          </button>
        </div>
      )}
      <button
        className="button"
        disabled={checking || pending}
        onClick={() => void action()}
      >
        {label}
      </button>
      {hasRetainedFailures && failed > 0 && (
        <button
          className="button"
          disabled={checking || pending || retryKind !== null}
          onClick={() => void retryFailed()}
        >
          Retry failed downloads
        </button>
      )}
    </section>
  );
}
