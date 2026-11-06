import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { Link, router } from "@inertiajs/react";

import { usePlayer } from "../../playback/components/PlaybackProvider";
import { useMutation } from "../../../shared/hooks/useMutation";
import EmptyState from "../../../shared/ui/EmptyState";
import FilterPills from "../../../shared/ui/FilterPills";
import SearchField from "../../../shared/ui/SearchField";
import {
  isApplePlatform,
  SHELL_SEARCH_ID,
  shortcutLabel,
} from "../../../app/searchShortcut";
import {
  AlbumResults,
  ArtistResults,
  TopResult,
  TrackResults,
} from "../components/LibraryResults";
import ProviderResultsSection from "../components/ProviderResultsSection";
import { useProviderSearch } from "../hooks/useProviderSearch";
import { useAutomaticLookups } from "../../metadata/hooks/useAutomaticLookups";
import { artistHits, topArtistHit } from "../model/artistHits";
import { topResultKind } from "../model/ranking";

const FEW_LIBRARY_RESULTS = 3;

type Scope = "all" | "library" | "providers";

export default function SearchPage({
  user,
  playlists,
  query,
  recordings,
  releases,
  artists,
  artist_groups,
  connections,
  service_names,
}: Schema<"SearchPageProps">) {
  const player = usePlayer();
  const { mutate } = useMutation(user.id);
  const [draft, setDraft] = useState(query);
  const [scope, setScope] = useState<Scope>("all");
  const [asked, setAsked] = useState<string | null>(null);
  const automatic =
    useAutomaticLookups(user.id).choices?.automatic_service_search ?? false;
  const artistResults = artistHits(artist_groups.items, artists.items);
  const libraryTotal =
    recordings.items.length + releases.items.length + artistResults.length;
  const sentAutomatically =
    automatic && query !== "" && libraryTotal < FEW_LIBRARY_RESULTS;
  const requested =
    scope !== "library" &&
    (scope === "providers" ||
      (asked === query && query !== "") ||
      sentAutomatically);
  const providers = useProviderSearch(
    user.id,
    connections,
    query,
    requested,
    player,
  );
  const library = scope !== "providers";
  const found = libraryTotal > 0;
  const topKind = topResultKind(query, {
    tracks: recordings.items.length,
    releases: releases.items.length,
    artistNames: artistResults.map((artist) => artist.name),
  });
  return (
    <div className="search-page">
      <header className="search-header">
        <h1>Search</h1>
        <SearchField
          id={SHELL_SEARCH_ID}
          label="Search"
          placeholder="Artist, album or track"
          value={draft}
          hint={shortcutLabel(isApplePlatform()).text}
          onChange={setDraft}
          onSubmit={(value) => router.get("/search", { q: value })}
        />
        {automatic && scope !== "library" && connections.length > 0 && (
          <p className="muted">
            With few matches in your library, your search is sent to your
            connected services.{" "}
            <Link className="text-link" href="/settings?section=privacy">
              Change in Settings
            </Link>
          </p>
        )}
        <FilterPills<Scope>
          label="Search scope"
          options={[
            { value: "all", label: "All" },
            { value: "library", label: "Library" },
            { value: "providers", label: "Providers" },
          ]}
          value={scope}
          onChange={setScope}
        />
      </header>
      {!query ? (
        <EmptyState
          title="Search your library and connected services"
          text="Find an artist, album or track."
          libraryLink={false}
        />
      ) : (
        <>
          {library && found && (
            <>
              <TopResult
                track={topKind === "track" ? recordings.items[0] : undefined}
                release={topKind === "release" ? releases.items[0] : undefined}
                artist={
                  topKind === "artist"
                    ? topArtistHit(query, artistResults)
                    : undefined
                }
              />
              <TrackResults
                tracks={recordings.items}
                playlists={playlists}
                mutate={mutate}
              />
              <AlbumResults releases={releases.items} />
              <ArtistResults artists={artistResults} />
            </>
          )}
          {library && !found && (
            <EmptyState
              title="No matches in your library"
              text="Try another name, or search your connected services."
              libraryLink={false}
            />
          )}
          {scope !== "library" && (
            <ProviderResultsSection
              search={providers}
              names={service_names}
              query={query}
              full={scope === "providers"}
              requested={requested}
              automatic={sentAutomatically && scope !== "providers"}
              onRequest={() => setAsked(query)}
              onSeeAll={() => setScope("providers")}
            />
          )}
        </>
      )}
      {scope === "providers" && !query && (
        <p className="home-hint">
          Browse without a search in{" "}
          <Link className="text-link" href="/discover">
            Discover
          </Link>
          .
        </p>
      )}
    </div>
  );
}
