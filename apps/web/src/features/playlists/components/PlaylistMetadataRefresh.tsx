import type { ImportedPlaylistMetadataRefresh } from "../../../shared/types";

type MetadataRefreshProps = {
  pending: boolean;
  hasAttempt: boolean;
  receipt: ImportedPlaylistMetadataRefresh | null;
  error: string;
  detailError: string;
  detailsPending: boolean;
  reloadDetails(): void;
  refresh(): Promise<void>;
  retry(): Promise<void>;
};

export default function PlaylistMetadataRefresh({
  pending,
  hasAttempt,
  receipt,
  error,
  detailError,
  detailsPending,
  reloadDetails,
  refresh,
  retry,
}: MetadataRefreshProps) {
  return (
    <section aria-label="Playlist metadata">
      <h2>Playlist metadata</h2>
      <p>
        Find missing album information through this playlist's original
        connection.
      </p>
      <button
        className="button"
        disabled={pending || hasAttempt}
        onClick={() => void refresh()}
      >
        Find missing albums
      </button>
      {hasAttempt && (
        <button
          className="button"
          disabled={pending}
          onClick={() => void retry()}
        >
          Retry album lookup
        </button>
      )}
      {receipt && (
        <p role="status">
          {receipt.updated_recordings}{" "}
          {receipt.updated_recordings === 1 ? "recording" : "recordings"}{" "}
          updated. {receipt.unchanged_recordings} unchanged.{" "}
          {receipt.unavailable_recordings} unavailable.
        </p>
      )}
      {error && <p role="alert">{error}</p>}
      {detailError && (
        <>
          <p role="alert">{detailError}</p>
          <button
            className="button"
            disabled={pending || detailsPending}
            onClick={reloadDetails}
          >
            Reload playlist details
          </button>
        </>
      )}
    </section>
  );
}
