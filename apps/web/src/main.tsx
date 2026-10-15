import { type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { createInertiaApp } from "@inertiajs/react";
import CatalogPage from "./pages/CatalogPage";
import HistoryPage from "./pages/HistoryPage";
import LikesPage from "./pages/LikesPage";
import LoginPage from "./pages/LoginPage";
import PlaylistIndexPage from "./pages/PlaylistIndexPage";
import PlaylistPage from "./pages/PlaylistPage";
import SettingsPage from "./pages/SettingsPage";
import ConnectionErrorPage from "./pages/ConnectionErrorPage";
import Shell from "./Shell";
import { installSessionBoundary } from "./session";
import "./style.css";

const layout = (page: ReactNode) => <Shell>{page}</Shell>;
const pages = {
  LoginPage,
  CatalogPage: Object.assign(CatalogPage, { layout }),
  LikesPage: Object.assign(LikesPage, { layout }),
  PlaylistIndexPage: Object.assign(PlaylistIndexPage, { layout }),
  PlaylistPage: Object.assign(PlaylistPage, { layout }),
  HistoryPage: Object.assign(HistoryPage, { layout }),
  SettingsPage: Object.assign(SettingsPage, { layout }),
};
void createInertiaApp({
  resolve: (name) => {
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
