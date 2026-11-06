import { useState, type ReactNode } from "react";
import FallbackCover from "../../../shared/ui/FallbackCover";
import { useArtworkRetry } from "../../../shared/hooks/useArtworkRetry";
import { coverArtUrl } from "../api/coverArtApi";
import { isMusicBrainzId } from "../model/musicbrainzId";

const RETRY_MS = [2000, 5000];

export type ArtistPhotoKind = "artist" | "credited-artist";

export default function ArtistPhoto({
  kind = "artist",
  artist,
  identity,
  name,
  enabled = true,
  onImageError,
  children,
}: {
  kind?: ArtistPhotoKind;
  artist: string;
  identity: string;
  name: string;
  enabled?: boolean;
  onImageError?: () => void;
  children?: ReactNode;
}) {
  const src = coverArtUrl(kind, artist);
  const retry = useArtworkRetry(RETRY_MS, src);
  const [loaded, setLoaded] = useState(false);
  const unavailable =
    retry.exhausted ||
    !enabled ||
    (kind === "artist" && !isMusicBrainzId(artist));
  if (unavailable && children) return <>{children}</>;
  const showing = !unavailable && !retry.waiting;
  return (
    <div
      className={`artwork ${showing ? "has-image" : ""}`}
      data-loaded={loaded ? "" : undefined}
      aria-hidden="true"
    >
      {!loaded && (
        <FallbackCover id={identity} title={name} kind="artist" monogramOnly />
      )}
      {showing && (
        <img
          key={retry.attempt}
          src={src}
          alt=""
          loading="lazy"
          onLoad={() => setLoaded(true)}
          onError={() => {
            onImageError?.();
            retry.miss();
          }}
        />
      )}
    </div>
  );
}
