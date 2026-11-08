import { useArtwork } from "./ArtworkProvider";
import type { ArtworkSubject } from "./ArtworkRetryMenu";
import Button from "../../../shared/ui/Button";

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
      <p role="status">{message}</p>
      <Button onClick={artwork.retry}>Retry artwork</Button>
    </div>
  );
}
