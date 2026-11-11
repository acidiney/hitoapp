import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { Link } from "@inertiajs/react";

import { usePlayer } from "../../playback/components/PlaybackProvider";
import { useMutation } from "../../../shared/hooks/useMutation";
import Button from "../../../shared/ui/Button";
import EmptyState from "../../../shared/ui/EmptyState";
import {
  AlbumResults,
  ArtistResults,
  TrackResults,
} from "../components/LibraryResults";
import ProviderResultsSection from "../components/ProviderResultsSection";
import SearchBar, { SearchHints } from "../components/SearchBar";
import SearchFilters, { type Scope } from "../components/SearchFilters";
import TopResult, { topResult } from "../components/TopResult";
import { useLiveQuery } from "../hooks/useLiveQuery";
import { useProviderSearch } from "../hooks/useProviderSearch";
import { useResultKeys } from "../hooks/useResultKeys";
import { useAutomaticLookups } from "../../metadata/hooks/useAutomaticLookups";
import { useArtistRadio } from "../../discovery/hooks/useArtistRadio";
import { artistHits } from "../model/artistHits";
import { topResultKind } from "../model/ranking";

const FEW_LIBRARY_RESULTS = 3;
const TRACK_PREVIEW = 8;

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
  const radio = useArtistRadio(user.id, player);
  const live = useLiveQuery(query);
  const keys = useResultKeys();
  const [scope, setScope] = useState<Scope>("all");
  const [sendingTo, setSendingTo] = useState<string | null | false>(false);
  const automatic =
    useAutomaticLookups(user.id).choices?.automatic_service_search ?? false;
  const artistResults = artistHits(artist_groups.items, artists.items);
  const libraryTotal =
    recordings.items.length + releases.items.length + artistResults.length;
  const sentAutomatically =
    automatic && query !== "" && libraryTotal < FEW_LIBRARY_RESULTS;
  const requested =
    scope !== "albums" &&
    scope !== "artists" &&
    query !== "" &&
    (sendingTo !== false || sentAutomatically);
  const providers = useProviderSearch(
    user.id,
    connections,
    query,
    requested,
    player,
  );
  const sending = requested && providers.selected !== null;
  const found = libraryTotal > 0;
  const topKind = topResultKind(query, {
    tracks: recordings.items.length,
    releases: releases.items.length,
    artistNames: artistResults.map((artist) => artist.name),
  });
  const top = topResult(
    topKind,
    artistResults,
    artist_groups.items,
    releases.items,
    recordings.items,
  );
  function toggleService(provider: Schema<"PluginInstallation">) {
    const on = sending && providers.selected?.id === provider.id;
    if (on) {
      setSendingTo(false);
      return;
    }
    providers.choose(provider.id);
    setSendingTo(provider.id);
    if (scope === "albums" || scope === "artists") setScope("all");
  }
  function playTop() {
    if (top?.kind === "track") void player.playQueue([top.track]);
    else if (recordings.items.length > 0)
      void player.playQueue(recordings.items);
  }
  const showTop = scope === "all" && top !== null;
  const showTracks = scope === "all" || scope === "tracks";
  const showAlbums = scope === "all" || scope === "albums";
  const showArtists = scope === "artists" || (scope === "all" && !showTop);
  const otherArtists =
    scope === "all" && top?.kind === "artist"
      ? artistResults.slice(1)
      : artistResults;
  return (
    <div className="search-page">
      <h1 className="sr-only">Search</h1>
      <header className="search-header">
        <SearchBar
          value={live.draft}
          onChange={live.change}
          onSubmit={live.submit}
        />
        <SearchHints>
          {automatic && connections.length > 0 && (
            <span>
              With few matches in your library, your search is sent to your
              connected services.{" "}
              <Link className="text-link" href="/settings?section=privacy">
                Change in Settings
              </Link>
            </span>
          )}
        </SearchHints>
        <SearchFilters
          scope={scope}
          onScope={setScope}
          providers={providers.providers}
          selected={providers.selected?.id ?? null}
          sending={sending}
          names={service_names}
          onToggle={toggleService}
        />
      </header>
      <div className="search-results" ref={keys.results} onKeyDown={keys.move}>
        {!query ? (
          <EmptyState
            title="Search your library and connected services"
            text="Find an artist, album or track."
            libraryLink={false}
          />
        ) : (
          <>
            {found ? (
              <>
                <div className="search-lead">
                  {showTop && top && (
                    <section
                      className="search-group search-top"
                      aria-labelledby="search-top"
                    >
                      <div className="search-group-head">
                        <h2 id="search-top">Top result</h2>
                      </div>
                      <TopResult
                        result={top}
                        onPlay={playTop}
                        onRadio={(group) => void radio.start(group)}
                        radioBusy={radio.pending !== null}
                      />
                    </section>
                  )}
                  {showTracks && (
                    <TrackResults
                      tracks={recordings.items}
                      total={recordings.total}
                      limit={scope === "all" ? TRACK_PREVIEW : null}
                      playlists={playlists}
                      mutate={mutate}
                      onShowAll={() => setScope("tracks")}
                    />
                  )}
                </div>
                {showAlbums && <AlbumResults releases={releases.items} />}
                {(showArtists ||
                  (scope === "all" && otherArtists.length > 0)) && (
                  <ArtistResults
                    artists={showArtists ? artistResults : otherArtists}
                  />
                )}
              </>
            ) : (
              <EmptyState
                title={`Nothing in your library matches “${query}”`}
                text={
                  providers.selected && !sending
                    ? "Check the spelling or try fewer words. You can also turn on a connected service above."
                    : "Check the spelling or try fewer words. Connect a music service to search beyond your library."
                }
                libraryLink={false}
              >
                {providers.selected && !sending ? (
                  <Button
                    variant="primary"
                    onClick={() => toggleService(providers.selected!)}
                  >
                    Search{" "}
                    {service_names[providers.selected.service_id] ??
                      providers.selected.manifest.name}
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
            {sending && (
              <ProviderResultsSection
                search={providers}
                names={service_names}
                full={scope === "tracks"}
                automatic={sendingTo === false && sentAutomatically}
                onSeeAll={() => setScope("tracks")}
              />
            )}
          </>
        )}
      </div>
      {radio.notice && (
        <p
          className="radio-notice"
          role={radio.notice.tone === "error" ? "alert" : "status"}
          data-tone={radio.notice.tone}
        >
          {radio.notice.text}
        </p>
      )}
    </div>
  );
}
