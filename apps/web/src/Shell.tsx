import { useEffect, type ReactNode } from "react";
import { usePage } from "@inertiajs/react";
import type { PageProps } from "@inertiajs/core";
import type { SharedPageProps } from "./shared/types";
import { log } from "./shared/log";
import LibrarySearch from "./components/LibrarySearch";
import Sidebar from "./components/Sidebar";
import { NoticeBanner, NoticeProvider } from "./shared/ui/Notices";
import { PlaybackProvider } from "./features/playback/components/PlaybackProvider";
import PlayerDock from "./features/playback/components/PlayerDock";
import { ArtworkProvider } from "./features/artwork/components/ArtworkProvider";

export default function Shell({ children }: { children: ReactNode }) {
  const page = usePage<SharedPageProps & PageProps>();
  const { user, playlists } = page.props;
  const url = new URL(page.url, "http://hito.local");
  useEffect(() => {
    log.info("navigation.page", { page: page.component, account_id: user.id });
  }, [page.component, page.url, user.id]);
  return (
    <NoticeProvider key={user.id}>
      <ArtworkProvider account={user.id}>
        <PlaybackProvider account={user.id} native={page.props.native_playback}>
          <a className="skip-link" href="#main">
            Skip to library
          </a>
          <div
            className={`app-shell ${page.component === "SettingsPage" ? "wide-workspace" : ""}`}
          >
            <Sidebar user={user} playlists={playlists} path={url.pathname} />
            <div className="workspace">
              {page.component !== "DiscoverPage" && (
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
              <NoticeBanner />
              <main id="main" tabIndex={-1}>
                {children}
              </main>
            </div>
          </div>
          <PlayerDock showListeningPanel={page.component !== "SettingsPage"} />
        </PlaybackProvider>
      </ArtworkProvider>
    </NoticeProvider>
  );
}
