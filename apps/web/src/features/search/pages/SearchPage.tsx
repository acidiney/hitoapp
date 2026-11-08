import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { Link, router } from "@inertiajs/react";

import { usePlayer } from "../../playback/components/PlaybackProvider";
import { useMutation } from "../../../shared/hooks/useMutation";
import Button from "../../../shared/ui/Button";
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
  TrackResults,
} from "../components/LibraryResults";
import ProviderResultsSection from "../components/ProviderResultsSection";
import { useProviderSearch } from "../hooks/useProviderSearch";
import { useAutomaticLookups } from "../../metadata/hooks/useAutomaticLookups";
import { artistHits } from "../model/artistHits";
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
  const askable = providers.selected !== null && !requested;
  function askServices() {
    if (scope === "library") setScope("all");
    setAsked(query);
  }
  const tracksSection = (
    <TrackResults
      key="tracks"
      tracks={recordings.items}
      total={recordings.total}
      playlists={playlists}
      mutate={mutate}
    />
  );
  const albumsSection = (
    <AlbumResults
      key="albums"
      releases={releases.items}
      total={releases.total}
    />
  );
  const artistsSection = (
    <ArtistResults key="artists" artists={artistResults} />
  );
  const sections =
    topKind === "artist"
      ? [artistsSection, tracksSection, albumsSection]
      : topKind === "release"
        ? [albumsSection, tracksSection, artistsSection]
        : [tracksSection, albumsSection, artistsSection];
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
          {library && found && sections}
          {library && !found && (
            <EmptyState
              title={`Nothing in your library matches “${query}”`}
              text={
                askable
                  ? "Check the spelling or try fewer words. You can also search your connected services."
                  : "Check the spelling or try fewer words. Connect a music service to search beyond your library."
              }
              libraryLink={false}
            >
              {askable ? (
                <Button variant="primary" onClick={askServices}>
                  Search your services
                </Button>
              ) : connections.length === 0 ? (
                <Link
                  className="button primary"
                  href="/settings?section=plugins"
                >
                  Connect a music service
                </Link>
              ) : null}
            </EmptyState>
          )}
          {scope !== "library" && (
            <ProviderResultsSection
              search={providers}
              names={service_names}
              query={query}
              full={scope === "providers"}
              requested={requested}
              automatic={sentAutomatically && scope !== "providers"}
              prompt={found || scope === "providers"}
              onRequest={askServices}
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
