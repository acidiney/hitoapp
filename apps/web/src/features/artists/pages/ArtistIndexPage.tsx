import type { Schema } from "../../../shared/schema";
import { Link, usePage } from "@inertiajs/react";

import Artwork from "../../artwork/components/Artwork";
import PageHeading from "../../../shared/ui/PageHeading";
import EmptyState from "../../../shared/ui/EmptyState";
import Pagination from "../../../shared/ui/Pagination";
import ArtistMatchingStatus from "../components/ArtistMatchingStatus";

export default function ArtistIndexPage({
  user,
  artists,
  identity_groups,
  libraries,
  query,
}: Schema<"ArtistIndexPageProps">) {
  const { url } = usePage();
  const current = new URL(url, window.location.origin).searchParams;
  const filters = new URLSearchParams();
  const library = current.get("library");
  if (library && libraries.some((item) => item.id === library))
    filters.set("library", library);
  const limit = current.get("limit");
  if (limit) filters.set("limit", limit);
  const suffix = filters.size ? `?${filters}` : "";
  return (
    <>
      <PageHeading
        title="Artists"
        description={`${artists.total} ${artists.total === 1 ? "artist credit" : "artist credits"} · ${identity_groups.total} chosen`}
      />
      <ArtistMatchingStatus account={user.id} />
      <section aria-labelledby="chosen-artists-title">
        <h2 className="discography-heading" id="chosen-artists-title">
          Chosen artists
        </h2>
        {identity_groups.items.length ? (
          <div className="artist-grid">
            {identity_groups.items.map((group) => (
              <Link
                className="artist-card round"
                key={group.id}
                href={`/artist-groups/${encodeURIComponent(group.id)}${suffix}`}
              >
                <Artwork
                  recording={group.name.recording_id}
                  fallback={{ id: group.id, title: group.name.value }}
                />
                <h2>{group.name.value}</h2>
                <small>
                  {group.recording_count}{" "}
                  {group.recording_count === 1 ? "track" : "tracks"}
                  {" · "}
                  {group.library_count}{" "}
                  {group.library_count === 1 ? "library" : "libraries"}
                </small>
              </Link>
            ))}
          </div>
        ) : (
          <EmptyState
            title={
              query ? "No matching chosen artists" : "No chosen artists yet"
            }
            text="Choose a reviewed artist in recording details to group its selected tracks."
            libraryLink={false}
          />
        )}
        <Pagination
          page={identity_groups}
          query={query}
          path="/artists"
          parameter="group_offset"
          label="Chosen artist pages"
        />
      </section>
      <section aria-labelledby="library-credits-title">
        <h2 className="discography-heading" id="library-credits-title">
          Library credits
        </h2>
        {artists.items.length ? (
          <div className="artist-grid">
            {artists.items.map((artist) => (
              <Link
                className="artist-card round"
                key={artist.id}
                href={`/artists/${encodeURIComponent(artist.id)}`}
              >
                <Artwork
                  recording={artist.artwork_recording_id ?? undefined}
                  fallback={{ id: artist.id, title: artist.name }}
                />
                <h2>{artist.name}</h2>
                <small>
                  {artist.recording_count > 0 || artist.release_count === 0 ? (
                    <>
                      {artist.recording_count}{" "}
                      {artist.recording_count === 1 ? "track" : "tracks"}
                      {artist.release_count > 0 && " · "}
                    </>
                  ) : null}
                  {artist.release_count > 0 && (
                    <>
                      {artist.release_count}{" "}
                      {artist.release_count === 1 ? "album" : "albums"}
                    </>
                  )}
                </small>
              </Link>
            ))}
          </div>
        ) : (
          <EmptyState
            title={query ? "No matching artists" : "Meet your collection"}
            text={
              query
                ? "Try another artist name."
                : "Artists appear here when you add recordings to your library."
            }
            libraryLink
          />
        )}
        <Pagination page={artists} query={query} path="/artists" />
      </section>
    </>
  );
}
