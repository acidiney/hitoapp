import type { Schema } from "../shared/schema";
import { useEffect, type ReactNode } from "react";
import { usePage } from "@inertiajs/react";
import type { PageProps } from "@inertiajs/core";

import { log } from "../shared/log";
import AccountMenu from "./AccountMenu";
import LibrarySearch from "./LibrarySearch";
import { handleSearchShortcut, isApplePlatform } from "./searchShortcut";
import LibrarySections from "./LibrarySections";
import Sidebar from "./Sidebar";
import ConnectionNotice from "../shared/ui/ConnectionNotice";
import { NoticeBanner, NoticeProvider } from "../shared/ui/Notices";
import { PlaybackProvider } from "../features/playback/components/PlaybackProvider";
import PlayerDock from "../features/playback/components/PlayerDock";
import { ArtworkProvider } from "../features/artwork/components/ArtworkProvider";

const OWN_SEARCH = new Set([
  "HomePage",
  "SearchPage",
  "DiscoverPage",
  "GenrePage",
  "SettingsPage",
  "QueuePage",
]);

export default function Shell({ children }: { children: ReactNode }) {
  const page = usePage<Schema<"SharedPageProps"> & PageProps>();
  const { user, playlists, lock } = page.props;
  const url = new URL(page.url, "http://hito.local");
  useEffect(() => {
    log.info("navigation.page", { page: page.component, account_id: user.id });
  }, [page.component, page.url, user.id]);
  useEffect(() => {
    const apple = isApplePlatform();
    const listen = (event: KeyboardEvent) => handleSearchShortcut(event, apple);
    window.addEventListener("keydown", listen);
    return () => window.removeEventListener("keydown", listen);
  }, []);
  return (
    <NoticeProvider key={user.id}>
      <ArtworkProvider account={user.id}>
        <PlaybackProvider account={user.id} native={page.props.native_playback}>
          <a className="skip-link" href="#main">
            Skip to content
          </a>
          <div
            className={`app-shell ${page.component === "SettingsPage" ? "wide-workspace" : ""}`}
          >
            <Sidebar playlists={playlists} path={url.pathname} />
            <div className="workspace">
              <AccountMenu user={user} lock={lock} path={url.pathname} />
              {!OWN_SEARCH.has(page.component) && (
                <LibrarySearch
                  query={url.searchParams.get("q") ?? ""}
                  target={
                    page.component === "AlbumIndexPage"
                      ? "albums"
                      : page.component === "ArtistIndexPage"
                        ? "artists"
                        : "library"
                  }
                />
              )}
              <LibrarySections path={url.pathname} />
              <ConnectionNotice />
              <NoticeBanner />
              <main id="main" tabIndex={-1}>
                {children}
              </main>
            </div>
          </div>
          <PlayerDock
            account={user.id}
            queueActive={url.pathname === "/queue"}
          />
        </PlaybackProvider>
      </ArtworkProvider>
    </NoticeProvider>
  );
}
