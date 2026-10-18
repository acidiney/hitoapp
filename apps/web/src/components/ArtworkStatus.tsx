import { useArtwork } from "../ArtworkProvider";

type ArtworkStatusProps =
  | { recording: string; release?: never; playlistId?: never }
  | { recording?: never; release: string; playlistId?: never }
  | { recording?: never; release?: never; playlistId: string };

export default function ArtworkStatus({
  recording,
  release,
  playlistId,
}: ArtworkStatusProps) {
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
          : artwork.phase === "unavailable"
            ? `No artwork provider is available for this ${playlistId ? "playlist" : release ? "album" : "track"}.`
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
