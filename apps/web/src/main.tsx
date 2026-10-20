import { type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { createInertiaApp } from "@inertiajs/react";
import CatalogPage from "./pages/CatalogPage";
import DiscoverPage from "./pages/DiscoverPage";
import AlbumIndexPage from "./pages/AlbumIndexPage";
import AlbumPage from "./pages/AlbumPage";
import AlbumDetailsPage from "./pages/AlbumDetailsPage";
import ArtistIndexPage from "./pages/ArtistIndexPage";
import ArtistPage from "./pages/ArtistPage";
import ArtistIdentityGroupPage from "./pages/ArtistIdentityGroupPage";
import RecordingDetailsPage from "./pages/RecordingDetailsPage";
import HistoryPage from "./pages/HistoryPage";
import LikesPage from "./pages/LikesPage";
import LoginPage from "./pages/LoginPage";
import PlaylistIndexPage from "./pages/PlaylistIndexPage";
import AudiobookIndexPage from "./pages/AudiobookIndexPage";
import AudiobookPage from "./pages/AudiobookPage";
import PodcastIndexPage from "./pages/PodcastIndexPage";
import PodcastPage from "./pages/PodcastPage";
import PlaylistPage from "./pages/PlaylistPage";
import SettingsPage from "./pages/SettingsPage";
import ConnectionErrorPage from "./pages/ConnectionErrorPage";
import Shell from "./Shell";
import { installSessionBoundary } from "./session";
import { installHistoryPolicy, prepareHistoryPage } from "./historyPolicy";
import "./style.css";

const layout = (page: ReactNode) => <Shell>{page}</Shell>;
const pages = {
  LoginPage,
  CatalogPage: Object.assign(CatalogPage, { layout }),
  DiscoverPage: Object.assign(DiscoverPage, { layout }),
  AlbumIndexPage: Object.assign(AlbumIndexPage, { layout }),
  AlbumPage: Object.assign(AlbumPage, { layout }),
  AlbumDetailsPage: Object.assign(AlbumDetailsPage, { layout }),
  ArtistIndexPage: Object.assign(ArtistIndexPage, { layout }),
  ArtistPage: Object.assign(ArtistPage, { layout }),
  ArtistIdentityGroupPage: Object.assign(ArtistIdentityGroupPage, { layout }),
  RecordingDetailsPage: Object.assign(RecordingDetailsPage, { layout }),
  LikesPage: Object.assign(LikesPage, { layout }),
  PlaylistIndexPage: Object.assign(PlaylistIndexPage, { layout }),
  AudiobookIndexPage: Object.assign(AudiobookIndexPage, { layout }),
  AudiobookPage: Object.assign(AudiobookPage, { layout }),
  PodcastIndexPage: Object.assign(PodcastIndexPage, { layout }),
  PodcastPage: Object.assign(PodcastPage, { layout }),
  PlaylistPage: Object.assign(PlaylistPage, { layout }),
  HistoryPage: Object.assign(HistoryPage, { layout }),
  SettingsPage: Object.assign(SettingsPage, { layout }),
};
installHistoryPolicy();
void createInertiaApp({
  resolve: (name, page) => {
    prepareHistoryPage(page);
    if (!Object.hasOwn(pages, name)) throw new Error(`Unknown page: ${name}`);
    return pages[name as keyof typeof pages];
  },
  setup: async ({ el, App, props }) => {
    const root = createRoot(el);
    const session = installSessionBoundary(props.initialPage.props, () =>
      root.unmount(),
    );
    try {
      if (await session.ready) root.render(<App {...props} />);
    } catch {
      root.render(<ConnectionErrorPage />);
    }
  },
  progress: { color: "#ff7848" },
});
