import { createRoot } from "react-dom/client";
import { createInertiaApp } from "@inertiajs/react";
import ConnectionErrorPage from "./ConnectionErrorPage";
import { resolvePage } from "./pages";
import { installSessionBoundary } from "../features/session/session";
import { installHistoryPolicy, prepareHistoryPage } from "./historyPolicy";
import "../style.css";

installHistoryPolicy();
void createInertiaApp({
  resolve: (name, page) => {
    prepareHistoryPage(page);
    return resolvePage(name);
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
  progress: { color: "#2f4bff" },
});
