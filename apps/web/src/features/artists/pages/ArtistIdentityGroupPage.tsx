import type { Schema } from "../../../shared/schema";

import { Link, usePage } from "@inertiajs/react";
import { useMutation } from "../../../shared/hooks/useMutation";
import Artwork from "../../artwork/components/Artwork";
import CollectionHeader from "../../../shared/ui/CollectionHeader";
import Breadcrumbs from "../../../shared/ui/Breadcrumbs";
import { tintFor } from "../../../shared/ui/collectionTint";
import PlayShuffle from "../../playback/components/PlayShuffle";
import AlbumGrid from "../../catalog/components/AlbumGrid";
import EmptyState from "../../../shared/ui/EmptyState";
import Pagination from "../../../shared/ui/Pagination";
import TrackTable, {
  type TrackTableRow,
} from "../../playback/components/TrackTable";
import { artistGroupTracksListing } from "../api/trackListings";
import TrackToolbar from "../../playback/components/TrackToolbar";
import ArtistGroupDiscography from "../components/ArtistGroupDiscography";
import SimilarArtists from "../components/SimilarArtists";
import ArtistIdentityCorrections from "../components/ArtistIdentityCorrections";

export default function ArtistIdentityGroupPage({
  user,
  playlists,
  libraries: accessibleLibraries,
  detail,
}: Schema<"ArtistIdentityGroupPageProps">) {
  const { group, libraries, releases, recordings } = detail;
  const { mutate } = useMutation(user.id);
  const tracks = recordings.items.map(({ recording }) => ({
    ...recording,
    row_id: recording.id,
  }));
  const path = `/artist-groups/${encodeURIComponent(group.id)}`;
  const listing = artistGroupTracksListing(user.id, group.id, recordings);
  const { url } = usePage();
  const current = new URL(url, window.location.origin).searchParams;
  function libraryPath(library?: string) {
    const filters = new URLSearchParams();
    if (library) filters.set("library", library);
    const limit = current.get("limit");
    if (limit) filters.set("limit", limit);
    return filters.size ? `${path}?${filters}` : path;
  }
  return (
    <>
      <CollectionHeader
        tint={tintFor(group.id)}
        cover={
          <Artwork
            recording={group.name.recording_id}
            fallback={{ id: group.id, title: group.name.value }}
          />
        }
        size="artist"
        crumbs={
          <Breadcrumbs
            crumbs={[
              { label: "Library", href: "/library" },
              { label: "Artists", href: "/artists" },
              { label: group.name.value },
            ]}
          />
        }
        eyebrow="Chosen artist"
        title={group.name.value}
        stats={`${group.recording_count} ${group.recording_count === 1 ? "track" : "tracks"} · ${group.release_count} ${group.release_count === 1 ? "album" : "albums"} · ${group.library_count} ${group.library_count === 1 ? "library" : "libraries"}`}
        actions={
          tracks.length > 0 ? (
            <PlayShuffle tracks={tracks} listing={listing} />
          ) : undefined
        }
      />
      <ArtistGroupDiscography account={user.id} group={group} />
      <SimilarArtists account={user.id} group={group} />
      <section aria-labelledby="group-libraries-title">
        <h2 className="discography-heading" id="group-libraries-title">
          Libraries
        </h2>
        {current.has("library") && (
          <Link className="text-link" href={libraryPath()}>
            All libraries
          </Link>
        )}
        {libraries.items.length ? (
          <ul>
            {libraries.items.map((library) => (
              <li key={library.library_id}>
                <Link
                  className="text-link"
                  href={libraryPath(library.library_id)}
                  aria-current={
                    current.get("library") === library.library_id
                      ? "page"
                      : undefined
                  }
                >
                  {accessibleLibraries.find(
                    (item) => item.id === library.library_id,
                  )?.name || "Library"}
                </Link>
                {" · "}
                {library.recording_count}{" "}
                {library.recording_count === 1 ? "track" : "tracks"}
              </li>
            ))}
          </ul>
        ) : (
          <EmptyState
            title="No libraries on this page"
            text="Return to the previous page to browse this chosen artist's libraries."
            libraryLink={false}
          />
        )}
        <Pagination
          page={libraries}
          query=""
          path={path}
          parameter="library_offset"
          label="Library pages"
        />
      </section>
      {(releases.total > 0 || releases.offset > 0) && (
        <section aria-labelledby="group-albums-title">
          <h2 className="discography-heading" id="group-albums-title">
            Albums & appearances
          </h2>
          {releases.items.length ? (
            <AlbumGrid releases={releases.items} />
          ) : (
            <EmptyState
              title="No albums on this page"
              text="Return to the previous page to browse this chosen artist's collection."
              libraryLink={false}
            />
          )}
          <Pagination
            page={releases}
            query=""
            path={path}
            parameter="release_offset"
            label="Album pages"
          />
        </section>
      )}
      {(recordings.total > 0 || recordings.offset > 0) && (
        <section aria-labelledby="group-tracks-title">
          <h2 className="discography-heading" id="group-tracks-title">
            Tracks
          </h2>
          {tracks.length ? (
            <>
              <TrackToolbar
                tracks={tracks}
                listing={listing}
                withPlay={false}
                description="Tracks with your chosen artist identity"
              />
              <TrackTable
                rows={tracks.map<TrackTableRow>((track) => ({
                  kind: "recording",
                  track,
                }))}
                listing={listing}
                offset={recordings.offset}
                playlists={playlists}
                mutate={mutate}
              />
            </>
          ) : (
            <EmptyState
              title="No tracks on this page"
              text="Return to the previous page to listen to this chosen artist."
              libraryLink={false}
            />
          )}
          <Pagination
            page={recordings}
            query=""
            path={path}
            parameter="recording_offset"
            label="Track pages"
          />
        </section>
      )}
      <ArtistIdentityCorrections
        account={user.id}
        admin={user.admin}
        detail={detail}
      />
    </>
  );
}
