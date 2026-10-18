import { useArtwork } from "../ArtworkProvider";

type ArtworkStatusProps =
  | { recording: string; release?: never }
  | { recording?: never; release: string };

export default function ArtworkStatus({
  recording,
  release,
}: ArtworkStatusProps) {
  const artwork = useArtwork(
    release ? "release" : "recording",
    release ?? recording,
  );
  const message =
    artwork.phase === "failed"
      ? "Artwork could not be retrieved."
      : artwork.phase === "cancelled"
        ? "Artwork retrieval was cancelled."
        : artwork.phase === "unavailable"
          ? `No artwork provider is available for this ${release ? "album" : "track"}.`
          : null;
  if (!message) return null;
  return (
    <div className="artwork-status">
      <p role="status">{message}</p>
      <button className="button" onClick={artwork.retry}>
        Retry artwork
      </button>
    </div>
  );
}
