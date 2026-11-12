import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { router } from "@inertiajs/react";

import CatalogCover from "./CatalogCover";
import { releaseAvailability } from "../model/releaseAvailability";
import AvailabilityLabel from "../../../shared/ui/AvailabilityLabel";
import LibraryAvailability from "./LibraryAvailability";
import ArtistEditions from "./ArtistEditions";
import CatalogAlbum from "./CatalogAlbum";
import MediaCard from "../../../shared/ui/MediaCard";

function releaseSubtitle(group: Schema<"CatalogReleaseGroup">, kind: string) {
  const year = group.first_release_date?.slice(0, 4);
  const types = group.secondary_types.map((type, index) =>
    index === 0 ? type : type.toLowerCase(),
  );
  return [year, types.length ? types.join(", ") : kind]
    .filter(Boolean)
    .join(" · ");
}

function libraryLine(albums: Schema<"LibraryAlbum">[]) {
  const best = albums.reduce((a, b) =>
    b.playable_count > a.playable_count ? b : a,
  );
  const count = `${best.track_count} ${best.track_count === 1 ? "track" : "tracks"} in your library`;
  return best.playable_count === best.track_count
    ? count
    : `${count} · ${best.playable_count} playable`;
}

const kinds = {
  album: "Album",
  ep: "EP",
  single: "Single",
  broadcast: "Broadcast",
  other: "Other",
};

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
  albumHref,
}: {
  albumHref?: (id: string) => string;
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
          <LibraryAvailability
            account={account}
            albums={library}
            group={group.id}
          />
        )}
      </li>
    );
  const availability = library ? releaseAvailability(library) : null;
  const kind = group.kind ? kinds[group.kind] : "Release";
  if (release === undefined)
    return (
      <li className="release-card">
        <MediaCard
          title={group.title}
          subtitle={releaseSubtitle(group, kind)}
          detail={
            library &&
            library.length > 0 && (
              <span
                className="release-library source-status"
                data-status="ready"
                role="img"
                aria-label={
                  availability?.label
                    ? `In your library, ${availability.label.toLowerCase()}`
                    : "In your library"
                }
              >
                <span className="source-status-dot" aria-hidden="true" />
                {libraryLine(library)}
              </span>
            )
          }
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
          href={albumHref?.(group.id)}
          onOpen={() => setView("open")}
          onPlay={
            albumHref
              ? () => router.get(`${albumHref(group.id)}?play=1`)
              : () => setView("play")
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
        <LibraryAvailability
          account={account}
          albums={library}
          group={group.id}
        />
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
