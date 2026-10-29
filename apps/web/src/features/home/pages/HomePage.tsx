import { useState } from "react";
import { Link, router } from "@inertiajs/react";
import type { HomePageProps, UnheardOnAlbum } from "../../../shared/types";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import SearchField from "../../../shared/ui/SearchField";
import {
  isApplePlatform,
  SHELL_SEARCH_ID,
  shortcutLabel,
} from "../../../app/searchShortcut";
import { greeting, quickAccess } from "../quickAccess";
import QuickAccess from "../components/QuickAccess";
import UnheardNearby from "../components/UnheardNearby";
import HomeEmpty from "../components/HomeEmpty";

export default function HomePage({ user, playlists, home }: HomePageProps) {
  const player = usePlayer();
  const [query, setQuery] = useState("");
  const tiles = quickAccess(home, playlists);
  const empty = !home.has_music && tiles.length === 0;
  function listen(album: UnheardOnAlbum) {
    void player.playQueue(album.tracks);
  }
  return (
    <div className="home-page">
      <header className="home-header">
        <h1>{greeting(new Date().getHours(), user.username)}</h1>
        <SearchField
          id={SHELL_SEARCH_ID}
          label="Search"
          placeholder="Artist, album or track"
          value={query}
          hint={shortcutLabel(isApplePlatform()).text}
          onChange={setQuery}
          onSubmit={(value) => router.get("/search", { q: value })}
        />
      </header>
      {empty ? (
        <HomeEmpty admin={user.admin} />
      ) : (
        <>
          {tiles.length > 0 ? (
            <QuickAccess tiles={tiles} />
          ) : (
            <p className="home-hint">
              Play something and it will show up here.{" "}
              <Link className="text-link" href="/library">
                Open your library
              </Link>
            </p>
          )}
          {home.unheard.length > 0 && (
            <UnheardNearby albums={home.unheard} onListen={listen} />
          )}
          <p className="home-hint">
            Looking for something new?{" "}
            <Link className="text-link" href="/discover">
              Discover music from your services
            </Link>
          </p>
        </>
      )}
    </div>
  );
}
