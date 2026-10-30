import { useState } from "react";
import { Disc3, ListMusic } from "lucide-react";
import { useArtwork } from "./ArtworkProvider";
import FallbackCover from "../../../shared/ui/FallbackCover";

export type CoverIdentity = {
  id: string;
  title: string;
  artist?: string | null;
  inline?: boolean;
};

export default function Artwork({
  recording,
  release,
  playlistId,
  playlist = false,
  className = "",
  fallback,
}: {
  recording?: string;
  release?: string;
  playlistId?: string;
  playlist?: boolean;
  className?: string;
  fallback?: CoverIdentity | null;
}) {
  const releaseRetrieval = useArtwork("release", release);
  const recordingRetrieval = useArtwork("recording", recording);
  const playlistRetrieval = useArtwork("playlist", playlistId);
  const [failedRelease, setFailedRelease] = useState<string>();
  const [failedRecording, setFailedRecording] = useState<string>();
  const [failedPlaylist, setFailedPlaylist] = useState<string>();
  const [loadedImage, setLoadedImage] = useState<string>();
  const playlistUrl = playlistId
    ? `/api/v1/playlists/${encodeURIComponent(playlistId)}/artwork`
    : undefined;
  const releaseUrl = release
    ? `/api/v1/releases/${encodeURIComponent(release)}/artwork`
    : undefined;
  const recordingUrl = recording
    ? `/api/v1/recordings/${encodeURIComponent(recording)}/artwork`
    : undefined;
  const releaseKey = JSON.stringify([
    releaseRetrieval.generation,
    releaseUrl,
    releaseRetrieval.revision,
  ]);
  const playlistKey = JSON.stringify([
    playlistRetrieval.generation,
    playlistUrl,
    playlistRetrieval.revision,
  ]);
  const recordingKey = JSON.stringify([
    recordingRetrieval.generation,
    recordingUrl,
    recordingRetrieval.revision,
  ]);
  const url =
    playlistUrl && failedPlaylist !== playlistKey
      ? playlistUrl
      : releaseUrl && failedRelease !== releaseKey
        ? releaseUrl
        : recordingUrl && failedRecording !== recordingKey
          ? recordingUrl
          : undefined;
  const imageKey =
    url === playlistUrl
      ? playlistKey
      : url === recordingUrl
        ? recordingKey
        : releaseKey;
  const loaded = Boolean(url) && loadedImage === imageKey;
  const markMissing = () => {
    if (url === playlistUrl) {
      setFailedPlaylist(playlistKey);
      playlistRetrieval.missing();
    } else if (url === releaseUrl) {
      setFailedRelease(releaseKey);
      releaseRetrieval.missing();
    } else {
      setFailedRecording(recordingKey);
      recordingRetrieval.missing();
    }
  };
  return (
    <div
      className={`artwork ${url ? "has-image" : ""} ${className}`}
      data-loaded={loaded ? "" : undefined}
      aria-hidden="true"
    >
      {url && fallback && !loaded && (
        <FallbackCover {...fallback} artist={null} inline={false} />
      )}
      {url ? (
        <img
          key={imageKey}
          src={url}
          alt=""
          loading="lazy"
          onLoad={() => {
            setLoadedImage(imageKey);
            if (url === playlistUrl) playlistRetrieval.loaded();
            else if (url === releaseUrl) releaseRetrieval.loaded();
            else recordingRetrieval.loaded();
          }}
          onError={markMissing}
        />
      ) : fallback ? (
        <FallbackCover {...fallback} />
      ) : playlist ? (
        <ListMusic strokeWidth={1} />
      ) : (
        <Disc3 strokeWidth={0.8} />
      )}
    </div>
  );
}
