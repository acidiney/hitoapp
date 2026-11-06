import { Link } from "@inertiajs/react";
import type { TrackRow } from "./TrackTable";

export function artistHref(track: Pick<TrackRow, "artist_id">): string | null {
  return track.artist_id
    ? `/artists/${encodeURIComponent(track.artist_id)}`
    : null;
}

export function albumHref(track: Pick<TrackRow, "release_id">): string | null {
  return track.release_id
    ? `/albums/${encodeURIComponent(track.release_id)}`
    : null;
}

function Name({ text, href }: { text: string; href: string | null }) {
  return href ? (
    <Link className="track-name" href={href}>
      {text}
    </Link>
  ) : (
    <span className="track-name">{text}</span>
  );
}

export default function TrackNames({
  artist,
  artistHref: artistLink = null,
  album,
  albumHref: albumLink = null,
  className,
}: {
  artist?: string;
  artistHref?: string | null;
  album?: string | null;
  albumHref?: string | null;
  className?: string;
}) {
  if (!artist && !album) return className ? null : <>—</>;
  return (
    <span className={className}>
      {artist && (
        <span className="track-name-part" data-part="artist">
          <Name text={artist} href={artistLink} />
        </span>
      )}
      {album && (
        <span className="track-name-part" data-part="album">
          <Name text={album} href={albumLink} />
        </span>
      )}
    </span>
  );
}
