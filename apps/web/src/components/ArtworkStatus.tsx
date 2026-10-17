import { useRecordingArtwork } from "../ArtworkProvider";

export default function ArtworkStatus({ recording }: { recording: string }) {
  const artwork = useRecordingArtwork(recording);
  const message =
    artwork.phase === "failed"
      ? "Artwork could not be retrieved."
      : artwork.phase === "cancelled"
        ? "Artwork retrieval was cancelled."
        : artwork.phase === "unavailable"
          ? "No artwork provider is available for this track."
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
