import { Link } from "@inertiajs/react";
import RouteLink from "../../../shared/ui/RouteLink";
import type { Schema } from "../../../shared/schema";
import type { TrackRow } from "./TrackTable";

type CreditMember = Schema<"CreditMember">;

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

export type RoutedLinks = { className: string; onNavigate?: () => void };

function Name({
  text,
  href,
  routed,
}: {
  text: string;
  href: string | null;
  routed?: RoutedLinks;
}) {
  if (href && routed)
    return (
      <RouteLink
        className={routed.className}
        href={href}
        onNavigate={routed.onNavigate}
      >
        {text}
      </RouteLink>
    );
  return href ? (
    <Link className="track-name" href={href}>
      {text}
    </Link>
  ) : (
    <span className="track-name">{text}</span>
  );
}

function joinBefore(members: CreditMember[], index: number): string {
  const stored = members[index].join;
  if (stored) return stored;
  return index === members.length - 2 ? " & " : ", ";
}

function CreditLinks({
  members,
  routed,
}: {
  members: CreditMember[];
  routed?: RoutedLinks;
}) {
  const mains = members.filter((member) => member.role === "main");
  const featured = members.filter((member) => member.role === "featured");
  return (
    <>
      {mains.map((member, index) => (
        <span key={member.id}>
          <Name
            text={member.name}
            href={`/artists/${encodeURIComponent(member.id)}`}
            routed={routed}
          />
          {index < mains.length - 1 ? joinBefore(mains, index) : ""}
        </span>
      ))}
      {featured.length > 0 && " feat. "}
      {featured.map((member, index) => (
        <span key={member.id}>
          <Name
            text={member.name}
            href={`/artists/${encodeURIComponent(member.id)}`}
            routed={routed}
          />
          {index < featured.length - 1
            ? index === featured.length - 2
              ? " & "
              : ", "
            : ""}
        </span>
      ))}
    </>
  );
}

export function ArtistNames({
  artist,
  artistHref: artistLink = null,
  credited = null,
  routed,
}: {
  artist: string;
  artistHref?: string | null;
  credited?: CreditMember[] | null;
  routed?: RoutedLinks;
}) {
  return credited && credited.length > 0 ? (
    <CreditLinks members={credited} routed={routed} />
  ) : (
    <Name text={artist} href={artistLink} routed={routed} />
  );
}

export default function TrackNames({
  artist,
  artistHref: artistLink = null,
  credited = null,
  album,
  albumHref: albumLink = null,
  className,
}: {
  artist?: string;
  artistHref?: string | null;
  credited?: CreditMember[] | null;
  album?: string | null;
  albumHref?: string | null;
  className?: string;
}) {
  if (!artist && !album) return className ? null : <>—</>;
  return (
    <span className={className}>
      {artist && (
        <span className="track-name-part" data-part="artist">
          <ArtistNames
            artist={artist}
            artistHref={artistLink}
            credited={credited}
          />
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
