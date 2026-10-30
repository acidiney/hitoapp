import type { Schema } from "../../../shared/schema";

import Button from "../../../shared/ui/Button";

type MetadataRefreshProps = {
  pending: boolean;
  hasAttempt: boolean;
  receipt: Schema<"ImportedPlaylistMetadataRefresh"> | null;
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
      <Button disabled={pending || hasAttempt} onClick={() => void refresh()}>
        Find missing albums
      </Button>
      {hasAttempt && (
        <Button disabled={pending} onClick={() => void retry()}>
          Retry album lookup
        </Button>
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
          <Button disabled={pending || detailsPending} onClick={reloadDetails}>
            Reload playlist details
          </Button>
        </>
      )}
    </section>
  );
}
