import { useState } from "react";
import { Disc3, ListMusic } from "lucide-react";
import { useRecordingArtwork } from "../ArtworkProvider";

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
  const retrieval = useRecordingArtwork(recording);
  const [failedRelease, setFailedRelease] = useState<string>();
  const [failedRecording, setFailedRecording] = useState<string>();
  const releaseUrl = release
    ? `/api/v1/releases/${encodeURIComponent(release)}/artwork`
    : undefined;
  const recordingUrl = recording
    ? `/api/v1/recordings/${encodeURIComponent(recording)}/artwork`
    : undefined;
  const recordingKey = JSON.stringify([recordingUrl, retrieval.revision]);
  const url =
    releaseUrl && failedRelease !== releaseUrl
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
          key={url === recordingUrl ? recordingKey : url}
          src={url}
          alt=""
          loading="lazy"
          onError={() => {
            if (url === releaseUrl) setFailedRelease(url);
            else {
              setFailedRecording(recordingKey);
              retrieval.missing();
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
