import type { Schema } from "../../../shared/schema";
import { Link, usePage } from "@inertiajs/react";

import Artwork from "../../artwork/components/Artwork";
import ArtistGroupPhoto from "../components/ArtistGroupPhoto";
import ArtistPhoto from "../components/ArtistPhoto";
import PageHeading from "../../../shared/ui/PageHeading";
import EmptyState from "../../../shared/ui/EmptyState";
import LoadMore from "../../../shared/ui/LoadMore";
import { useInfinitePage } from "../../../shared/hooks/useInfinitePage";
import { artistListApi, type ArtistListFilter } from "../api/artistListApi";
import ArtistMatchingStatus from "../components/ArtistMatchingStatus";
import ArtistReviewQueue from "../components/ArtistReviewQueue";

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
  const filter: ArtistListFilter = {
    query,
    library: filters.get("library"),
    limit,
  };
  const scope = JSON.stringify(filter);
  const restart = new URLSearchParams();
  if (query) restart.set("q", query);
  filters.forEach((value, key) => restart.set(key, value));
  const restartHref = restart.size ? `/artists?${restart}` : "/artists";
  const chosen = useInfinitePage({
    memory: "artists-chosen",
    scope,
    initial: identity_groups,
    fetchPage: (offset, signal) =>
      artistListApi.groups(user.id, filter, offset, signal),
  });
  const credits = useInfinitePage({
    memory: "artists-credits",
    scope,
    initial: artists,
    fetchPage: (offset, signal) =>
      artistListApi.credits(user.id, filter, offset, signal),
  });
  return (
    <>
      <PageHeading
        title="Artists"
        description={`${artists.total} ${artists.total === 1 ? "artist credit" : "artist credits"} · ${identity_groups.total} chosen`}
      />
      <ArtistMatchingStatus account={user.id} />
      <ArtistReviewQueue
        account={user.id}
        refreshKey={`${artists.total}-${identity_groups.total}`}
      />
      <section aria-labelledby="chosen-artists-title">
        <h2 className="discography-heading" id="chosen-artists-title">
          Chosen artists
        </h2>
        {chosen.items.length ? (
          <div className="artist-grid">
            {chosen.items.map((group) => (
              <Link
                className="artist-card round"
                key={group.id}
                href={`/artist-groups/${encodeURIComponent(group.id)}${suffix}`}
              >
                <ArtistGroupPhoto account={user.id} group={group}>
                  <Artwork
                    recording={group.name.recording_id}
                    fallback={{ id: group.id, title: group.name.value }}
                  />
                </ArtistGroupPhoto>
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
        {chosen.items.length === 0 && identity_groups.total > 0 && (
          <Link className="text-link" href={restartHref}>
            Show the first chosen artists
          </Link>
        )}
        <LoadMore
          {...chosen}
          loaded={chosen.items.length}
          noun="chosen artists"
          onLoad={() => void chosen.loadMore()}
        />
      </section>
      <section aria-labelledby="library-credits-title">
        <h2 className="discography-heading" id="library-credits-title">
          Library credits
        </h2>
        {credits.items.length ? (
          <div className="artist-grid">
            {credits.items.map((artist) => (
              <Link
                className="artist-card round"
                key={artist.id}
                href={`/artists/${encodeURIComponent(artist.id)}`}
              >
                <ArtistPhoto
                  kind="credited-artist"
                  artist={artist.id}
                  identity={artist.id}
                  name={artist.name}
                  enabled={artist.provider_photo === true}
                >
                  <Artwork
                    recording={artist.artwork_recording_id ?? undefined}
                    fallback={{ id: artist.id, title: artist.name }}
                  />
                </ArtistPhoto>
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
        {credits.items.length === 0 && artists.total > 0 && (
          <Link className="text-link" href={restartHref}>
            Show the first artists
          </Link>
        )}
        <LoadMore
          {...credits}
          loaded={credits.items.length}
          noun="artists"
          onLoad={() => void credits.loadMore()}
        />
      </section>
    </>
  );
}
