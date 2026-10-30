import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { Link, router } from "@inertiajs/react";

import { usePlayer } from "../../playback/components/PlaybackProvider";
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
import { useAutomaticServiceSearch } from "../model/autoSearch";
import { rankArtists, topResultKind } from "../model/ranking";

const FEW_LIBRARY_RESULTS = 3;

type Scope = "all" | "library" | "providers";

export default function SearchPage({
  user,
  query,
  recordings,
  releases,
  artists,
  connections,
  service_names,
}: Schema<"SearchPageProps">) {
  const player = usePlayer();
  const [draft, setDraft] = useState(query);
  const [scope, setScope] = useState<Scope>("all");
  const [asked, setAsked] = useState<string | null>(null);
  const [automatic] = useAutomaticServiceSearch(user.id);
  const libraryTotal =
    recordings.items.length + releases.items.length + artists.items.length;
  const sentAutomatically =
    automatic && query !== "" && libraryTotal < FEW_LIBRARY_RESULTS;
  const requested =
    scope === "providers" ||
    (asked === query && query !== "") ||
    sentAutomatically;
  const providers = useProviderSearch(
    user.id,
    connections,
    query,
    requested,
    player,
  );
  const library = scope !== "providers";
  const found = libraryTotal > 0;
  const rankedArtists = rankArtists(query, artists.items);
  const topKind = topResultKind(query, {
    tracks: recordings.items.length,
    releases: releases.items.length,
    artists: rankedArtists,
  });
  function play(track: Schema<"Recording">) {
    const playable = recordings.items.filter((item) => item.available);
    void player.playQueue(playable, Math.max(0, playable.indexOf(track)));
  }
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
                artist={topKind === "artist" ? rankedArtists[0] : undefined}
              />
              <TrackResults tracks={recordings.items} onPlay={play} />
              <AlbumResults releases={releases.items} />
              <ArtistResults artists={rankedArtists} />
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
