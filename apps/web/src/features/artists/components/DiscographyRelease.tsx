import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";

import FallbackCover from "../../../shared/ui/FallbackCover";
import { releaseAvailability } from "../model/releaseAvailability";
import AvailabilityLabel from "../../../shared/ui/AvailabilityLabel";
import ArtistEditions from "./ArtistEditions";

const kinds = {
  album: "Album",
  ep: "EP",
  single: "Single",
  broadcast: "Broadcast",
  other: "Other",
};

function tracks(count: number) {
  return `${count} ${count === 1 ? "track" : "tracks"}`;
}

function Availability({ albums }: { albums: Schema<"LibraryAlbum">[] }) {
  if (albums.length === 0) return <p>Not linked to a library album</p>;
  return (
    <ul aria-label="In your library">
      {albums.map((album) => (
        <li key={album.release_id}>
          <p>
            {album.playable_count === 0
              ? `${tracks(album.track_count)} in your library, none playable yet`
              : album.playable_count === album.track_count
                ? `${tracks(album.track_count)} in your library, all playable`
                : `${album.playable_count} of ${album.track_count} tracks playable`}
          </p>
          <Link
            className="text-link"
            href={`/albums/${encodeURIComponent(album.release_id)}`}
          >
            Open in your library
          </Link>
        </li>
      ))}
    </ul>
  );
}

export default function DiscographyRelease({
  account,
  recording,
  association,
  profile,
  group,
  library,
  heading: Heading = "h4",
  release,
  onAlbumEdition,
}: {
  account: string;
  recording: string;
  association: Schema<"ArtistAssociation">;
  profile: Schema<"CatalogArtistProfile">;
  group: Schema<"CatalogReleaseGroup">;
  library?: Schema<"LibraryAlbum">[];
  heading?: "h4" | "h5";
  release?: string;
  onAlbumEdition?: (edition: Schema<"SavedAlbumEdition"> | null) => void;
}) {
  return (
    <li className="release-card">
      <div className="release-cover">
        <FallbackCover
          id={group.id}
          title={group.title}
          artist={group.artist_credit?.[0]?.credited_name}
        />
      </div>
      {library && (
        <AvailabilityLabel
          state={releaseAvailability(library).state}
          label={releaseAvailability(library).label}
        />
      )}
      <Heading>{group.title}</Heading>
      <p>{group.kind ? kinds[group.kind] : "Type not provided"}</p>
      {group.secondary_types.length > 0 && (
        <p>{group.secondary_types.join(" · ")}</p>
      )}
      <p>
        {group.artist_credit?.length
          ? group.artist_credit
              .map((member) => member.credited_name + member.join_phrase)
              .join("")
          : "Artist credit not provided"}
      </p>
      <p>{group.first_release_date ?? "Release date not provided"}</p>
      {library && <Availability albums={library} />}
      <ArtistEditions
        account={account}
        recording={recording}
        association={association}
        profile={profile}
        group={group}
        release={release}
        onAlbumEdition={onAlbumEdition}
      />
    </li>
  );
}
