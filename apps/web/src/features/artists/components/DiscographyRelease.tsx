import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { Link } from "@inertiajs/react";

import CatalogCover from "./CatalogCover";
import { releaseAvailability } from "../model/releaseAvailability";
import AvailabilityLabel from "../../../shared/ui/AvailabilityLabel";
import AlbumLevelCheck from "./AlbumLevelCheck";
import ArtistEditions from "./ArtistEditions";
import CatalogAlbum from "./CatalogAlbum";
import MediaCard from "../../../shared/ui/MediaCard";

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

function Availability({
  account,
  albums,
  group,
}: {
  account: string;
  albums: Schema<"LibraryAlbum">[];
  group: string;
}) {
  if (albums.length === 0)
    return <p className="sr-only">Not linked to a library album</p>;
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
          <AlbumLevelCheck
            account={account}
            release={album.release_id}
            releaseGroup={group}
          />
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
  const [view, setView] = useState<"card" | "open" | "play">("card");
  if (release === undefined && view !== "card")
    return (
      <li className="release-card" data-open="">
        <CatalogAlbum
          context={{
            account,
            recording,
            association,
            profile,
            group,
            onAlbumEdition,
          }}
          heading={Heading}
          playOnReady={view === "play"}
        />
        {library && (
          <Availability account={account} albums={library} group={group.id} />
        )}
      </li>
    );
  const availability = library ? releaseAvailability(library) : null;
  const year = group.first_release_date?.slice(0, 4);
  const kind = group.kind ? kinds[group.kind] : "Release";
  if (release === undefined)
    return (
      <li className="release-card">
        <MediaCard
          title={group.title}
          subtitle={[kind, ...group.secondary_types, year]
            .filter(Boolean)
            .join(" · ")}
          heading={Heading}
          cover={
            <CatalogCover
              group={group.id}
              edition={null}
              title={group.title}
              artist={group.artist_credit?.[0]?.credited_name ?? null}
            />
          }
          openLabel={`Open ${group.title}`}
          onOpen={() => setView("open")}
          onPlay={() => setView("play")}
          inLibrary={
            availability?.state === "library"
              ? availability.label
                ? `In your library, ${availability.label.toLowerCase()}`
                : "In your library"
              : undefined
          }
        />
      </li>
    );
  return (
    <li className="release-card">
      <div className="release-cover">
        <CatalogCover
          group={group.id}
          edition={null}
          title={group.title}
          artist={group.artist_credit?.[0]?.credited_name ?? null}
        />
      </div>
      <Heading>{group.title}</Heading>
      <p className="release-credit">
        {group.artist_credit?.length
          ? group.artist_credit
              .map((member) => member.credited_name + member.join_phrase)
              .join("")
          : "Artist credit not provided"}
      </p>
      <p className="meta-line">
        <span className={group.kind ? "" : "placeholder"}>
          {group.kind ? kinds[group.kind] : "Type not provided"}
        </span>
        {group.secondary_types.map((type) => (
          <span key={type}>{type}</span>
        ))}
        <span className={group.first_release_date ? "" : "placeholder"}>
          {group.first_release_date ?? "Release date not provided"}
        </span>
      </p>
      {availability && (
        <AvailabilityLabel
          state={availability.state}
          label={
            availability.label ??
            (library?.length === 0 ? "Not in your library" : undefined)
          }
        />
      )}
      {library && (
        <Availability account={account} albums={library} group={group.id} />
      )}
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
