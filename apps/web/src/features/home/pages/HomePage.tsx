import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { Link, router } from "@inertiajs/react";

import { accountName } from "../../../shared/accountName";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import SearchField from "../../../shared/ui/SearchField";
import {
  isApplePlatform,
  SHELL_SEARCH_ID,
  shortcutLabel,
} from "../../../app/searchShortcut";
import { greeting, quickAccess } from "../model/quickAccess";
import QuickAccess from "../components/QuickAccess";
import UnheardNearby from "../components/UnheardNearby";
import HomeEmpty from "../components/HomeEmpty";
import HomeShelf from "../components/HomeShelf";
import { addedShelf, artistShelf, playedShelf } from "../model/homeShelves";

export default function HomePage({
  user,
  playlists,
  home,
  artists,
}: Schema<"HomePageProps">) {
  const player = usePlayer();
  const [query, setQuery] = useState("");
  const tiles = quickAccess(home, playlists);
  const empty = !home.has_music && tiles.length === 0;
  function listen(album: Schema<"UnheardOnAlbum">) {
    void player.playQueue(album.tracks);
  }
  return (
    <div className="home-page">
      <header className="home-header">
        <h1>{greeting(new Date().getHours(), accountName(user))}</h1>
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
          {tiles.length > 0 && <QuickAccess tiles={tiles} />}
          <HomeShelf
            id="home-played"
            title="Recently played"
            cards={playedShelf(home.recent_albums)}
            empty={
              <>
                Play something and it will show up here.{" "}
                <Link className="text-link" href="/library">
                  Open your library
                </Link>
              </>
            }
          />
          {home.unheard.length > 0 && (
            <UnheardNearby albums={home.unheard} onListen={listen} />
          )}
          {home.recently_added.length > 0 && (
            <HomeShelf
              id="home-added"
              title="Recently added"
              cards={addedShelf(home.recently_added)}
              more={{ href: "/albums", label: "See all albums" }}
            />
          )}
          {artists.items.length > 0 && (
            <HomeShelf
              id="home-artists"
              title="Your artists"
              cards={artistShelf(artists.items)}
              more={{ href: "/artists", label: "See all artists" }}
            />
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
