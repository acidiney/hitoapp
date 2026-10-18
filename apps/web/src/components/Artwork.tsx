import { useState } from "react";
import { Disc3, ListMusic } from "lucide-react";
import { useArtwork } from "../ArtworkProvider";

export default function Artwork({
  recording,
  release,
  playlist = false,
  className = "",
}: {
  recording?: string;
  release?: string;
  playlist?: boolean;
  className?: string;
}) {
  const releaseRetrieval = useArtwork("release", release);
  const recordingRetrieval = useArtwork("recording", recording);
  const [failedRelease, setFailedRelease] = useState<string>();
  const [failedRecording, setFailedRecording] = useState<string>();
  const releaseUrl = release
    ? `/api/v1/releases/${encodeURIComponent(release)}/artwork`
    : undefined;
  const recordingUrl = recording
    ? `/api/v1/recordings/${encodeURIComponent(recording)}/artwork`
    : undefined;
  const releaseKey = JSON.stringify([releaseUrl, releaseRetrieval.revision]);
  const recordingKey = JSON.stringify([
    recordingUrl,
    recordingRetrieval.revision,
  ]);
  const url =
    releaseUrl && failedRelease !== releaseKey
      ? releaseUrl
      : recordingUrl && failedRecording !== recordingKey
        ? recordingUrl
        : undefined;
  return (
    <div
      className={`artwork ${url ? "has-image" : ""} ${className}`}
      aria-hidden="true"
    >
      {url ? (
        <img
          key={url === recordingUrl ? recordingKey : releaseKey}
          src={url}
          alt=""
          loading="lazy"
          onError={() => {
            if (url === releaseUrl) {
              setFailedRelease(releaseKey);
              releaseRetrieval.missing();
            } else {
              setFailedRecording(recordingKey);
              recordingRetrieval.missing();
            }
          }}
        />
      ) : playlist ? (
        <ListMusic strokeWidth={1} />
      ) : (
        <Disc3 strokeWidth={0.8} />
      )}
    </div>
  );
}
