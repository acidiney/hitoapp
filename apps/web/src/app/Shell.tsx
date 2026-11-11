import type { Schema } from "../shared/schema";
import { useEffect, useState, type ReactNode } from "react";
import { usePage } from "@inertiajs/react";
import type { PageProps } from "@inertiajs/core";

import { log } from "../shared/log";
import AccountMenu from "./AccountMenu";
import ShellShortcuts from "./ShellShortcuts";
import LibrarySearch from "./LibrarySearch";
import { handleSearchShortcut, isApplePlatform } from "./searchShortcut";
import { searchPlacement } from "./searchPlacement";
import { PHONE_QUERY, useMediaQuery } from "../shared/hooks/useMediaQuery";
import Sidebar from "./Sidebar";
import ConnectionNotice from "../shared/ui/ConnectionNotice";
import { NoticeBanner, NoticeProvider } from "../shared/ui/Notices";
import { PlaybackProvider } from "../features/playback/components/PlaybackProvider";
import { TrackActionsProvider } from "../features/playback/components/TrackActionsProvider";
import PlayerDock from "../features/playback/components/PlayerDock";
import { ArtworkProvider } from "../features/artwork/components/ArtworkProvider";

function libraryTarget(component: string): "albums" | "artists" | "library" {
  if (component === "AlbumIndexPage") return "albums";
  return component === "ArtistIndexPage" ? "artists" : "library";
}

export default function Shell({ children }: { children: ReactNode }) {
  const page = usePage<Schema<"SharedPageProps"> & PageProps>();
  const { user, playlists, lock } = page.props;
  const url = new URL(page.url, "http://hito.local");
  const placement = searchPlacement(page.component, useMediaQuery(PHONE_QUERY));
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
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
          <TrackActionsProvider account={user.id} playlists={playlists}>
            <a className="skip-link" href="#main">
              Skip to content
            </a>
            <div
              className={`app-shell ${page.component === "SettingsPage" ? "wide-workspace" : ""}`}
            >
              <ShellShortcuts
                open={shortcutsOpen}
                onOpenChange={setShortcutsOpen}
              />
              <Sidebar
                playlists={playlists}
                path={url.pathname}
                search={url.search}
              />
              <div className="workspace">
                <AccountMenu
                  user={user}
                  lock={lock}
                  path={url.pathname}
                  onShowShortcuts={() => setShortcutsOpen(true)}
                />
                {placement === "global" && (
                  <LibrarySearch
                    query={
                      page.component === "SearchPage"
                        ? (url.searchParams.get("q") ?? "")
                        : ""
                    }
                    target="search"
                  />
                )}
                {placement === "library" && (
                  <LibrarySearch
                    query={url.searchParams.get("q") ?? ""}
                    target={libraryTarget(page.component)}
                  />
                )}
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
          </TrackActionsProvider>
        </PlaybackProvider>
      </ArtworkProvider>
    </NoticeProvider>
  );
}
