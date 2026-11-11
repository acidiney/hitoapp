import { useArtwork } from "./ArtworkProvider";
import type { ArtworkSubject } from "./ArtworkRetryMenu";

export default function ArtworkStatus({
  recording,
  release,
  playlistId,
}: ArtworkSubject) {
  const artwork = useArtwork(
    playlistId ? "playlist" : release ? "release" : "recording",
    playlistId ?? release ?? recording,
  );
  if (!artwork.imageFailed) return null;
  const message =
    artwork.phase === "ready"
      ? "Artwork could not be loaded."
      : artwork.phase === "failed"
        ? "Artwork could not be retrieved."
        : artwork.phase === "cancelled"
          ? "Artwork retrieval was cancelled."
          : null;
  if (!message) return null;
  return (
    <div className="artwork-status">
      <p role="status" className="source-status" data-status="failed">
        <span className="source-status-dot" aria-hidden="true" />
        {message}
      </p>
      <button
        type="button"
        className="text-action"
        aria-label="Retry artwork"
        onClick={artwork.retry}
      >
        Retry
      </button>
    </div>
  );
}
