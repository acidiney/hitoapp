import { router, type Page } from "@inertiajs/core";
import { log } from "../shared/log";

let freshHistory = false;

function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object";
}

function scrollPosition(value: unknown) {
  return object(value) &&
    typeof value.top === "number" &&
    Number.isFinite(value.top) &&
    typeof value.left === "number" &&
    Number.isFinite(value.left)
    ? { top: value.top, left: value.left }
    : undefined;
}

function scroll(state: unknown) {
  if (!object(state)) return {};
  return {
    documentScrollPosition: scrollPosition(state.documentScrollPosition),
    scrollRegions: Array.isArray(state.scrollRegions)
      ? state.scrollRegions.slice(0, 1000).flatMap((value: unknown) => {
          const position = scrollPosition(value);
          return position ? [position] : [];
        })
      : undefined,
  };
}

export function installHistoryPolicy() {
  if (freshHistory || typeof window.crypto.subtle !== "undefined") return;
  const push = window.history.pushState.bind(window.history);
  const replace = window.history.replaceState.bind(window.history);
  replace(scroll(window.history.state), "", window.location.href);

  function write(
    method: typeof push,
    state: unknown,
    unused: string,
    url?: string | URL | null,
  ) {
    method(
      { page: { hito_fresh_history: true }, ...scroll(state) },
      unused,
      url,
    );
  }

  window.history.pushState = (state, unused, url) =>
    write(push, state, unused, url);
  window.history.replaceState = (state, unused, url) =>
    write(replace, state, unused, url);
  window.addEventListener(
    "popstate",
    (event) => {
      event.stopImmediatePropagation();
      log.debug("history.fresh_page_requested", {
        reason: "history_navigation",
      });
      router.visit(window.location.href, {
        replace: true,
        preserveScroll: true,
        preserveState: true,
      });
    },
    { capture: true },
  );
  freshHistory = true;
  log.info("history.policy_selected", { mode: "fresh_page" });
}

export function prepareHistoryPage(page: Page | undefined) {
  if (freshHistory && page) page.encryptHistory = false;
}
