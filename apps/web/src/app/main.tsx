import { createRoot } from "react-dom/client";
import { createInertiaApp, router } from "@inertiajs/react";
import ConnectionErrorPage from "./ConnectionErrorPage";
import { resolvePage } from "./pages";
import { installConnectionRecovery } from "./connectionRecovery";
import { markPageLoaded } from "./reloadGuard";
import { installSessionBoundary } from "../features/session/api/session";
import { installHistoryPolicy, prepareHistoryPage } from "./historyPolicy";
import { currentEnvironment, registerServiceWorker } from "./serviceWorker";
import { showUpdateNotice } from "./UpdateNotice";
import "../style.css";

void registerServiceWorker(currentEnvironment(), showUpdateNotice);
installHistoryPolicy();
installConnectionRecovery(router);
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
      if (await session.ready) {
        markPageLoaded();
        root.render(<App {...props} />);
      }
    } catch {
      root.render(<ConnectionErrorPage />);
    }
  },
  progress: { color: "#2f4bff" },
});
