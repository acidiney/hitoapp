import { useEffect, useState, type ReactNode } from "react";
import FallbackCover from "../../../shared/ui/FallbackCover";
import { isMusicBrainzId } from "../model/musicbrainzId";

const MAX_RETRIES = 3;
const RETRY_MS = [2000, 5000, 12000];

function initials(name: string): string {
  const letters = name
    .split(/\s+/)
    .map((word) => Array.from(word.replace(/[^\p{L}\p{N}]/gu, ""))[0])
    .filter(Boolean)
    .slice(0, 2)
    .join("");
  return letters.toUpperCase() || "?";
}

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
  const [attempt, setAttempt] = useState(0);
  const [waiting, setWaiting] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const exhausted =
    attempt >= MAX_RETRIES ||
    !enabled ||
    (kind === "artist" && !isMusicBrainzId(artist));
  useEffect(() => {
    if (!waiting) return;
    const timer = setTimeout(
      () => {
        setAttempt((value) => value + 1);
        setWaiting(false);
      },
      RETRY_MS[Math.min(attempt, RETRY_MS.length - 1)],
    );
    return () => clearTimeout(timer);
  }, [waiting, attempt]);
  if (exhausted && children) return <>{children}</>;
  const showing = !exhausted && !waiting;
  return (
    <div
      className={`artwork ${showing ? "has-image" : ""}`}
      data-loaded={loaded ? "" : undefined}
      aria-hidden="true"
    >
      {!loaded && (
        <FallbackCover id={identity} title={name} monogram={initials(name)} />
      )}
      {showing && (
        <img
          key={attempt}
          src={`/api/v1/cover-art/${kind}/${encodeURIComponent(artist)}`}
          alt=""
          loading="lazy"
          onLoad={() => setLoaded(true)}
          onError={() => {
            onImageError?.();
            if (attempt + 1 >= MAX_RETRIES) setAttempt(MAX_RETRIES);
            else setWaiting(true);
          }}
        />
      )}
    </div>
  );
}
