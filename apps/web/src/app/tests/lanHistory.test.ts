// @vitest-environment-options {"url":"http://192.168.1.119:8080/history"}
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { http, router, type Page } from "@inertiajs/core";
import { notifySessionChanged } from "../../features/session/session";

const nativeState = Object.getOwnPropertyDescriptor(
  History.prototype,
  "state",
)!.get!;
const nativeReplace = History.prototype.replaceState;
const crypto = globalThis.crypto;
const subtle = Object.getOwnPropertyDescriptor(crypto, "subtle");
const account = { id: "alice", username: "Alice", admin: false };
const privateTitle = "Owned private LAN recording";
const privateArtist = "Owned private LAN artist";
const recording = {
  id: "af7bb5b5-f4a7-4443-a730-29c5985ca2a2",
  title: privateTitle,
  artist: privateArtist,
  duration_ms: 10000,
  position_ms: 0,
  speed: 1,
  available: true,
  liked: false,
};

function page(component: string, url: string): Page {
  return {
    component,
    url,
    version: "owned-lan-fixture",
    encryptHistory: true,
    clearHistory: false,
    rememberedState: {},
    rescuedProps: [],
    flash: {},
    props: {
      errors: {},
      user: account,
      native_playback: false,
      libraries: [],
      playlists: [],
      activity: { history: [recording] },
      query: "",
      recordings: { items: [], total: 0, offset: 0, limit: 60 },
    },
  };
}

afterEach(() => {
  router.cancelAll();
  if (subtle) Object.defineProperty(crypto, "subtle", subtle);
  else Reflect.deleteProperty(crypto, "subtle");
  document.body.innerHTML = "";
});

test("HTTP LAN navigation fetches fresh private pages through history while preserving playback", async () => {
  Object.defineProperty(crypto, "subtle", {
    configurable: true,
    value: undefined,
  });
  expect(crypto.subtle).toBeUndefined();
  localStorage.clear();
  sessionStorage.clear();
  vi.stubGlobal("BroadcastChannel", undefined);
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(function (
    this: HTMLMediaElement,
  ) {
    this.dispatchEvent(new Event("pause"));
  });
  vi.spyOn(HTMLMediaElement.prototype, "play").mockImplementation(
    async function (this: HTMLMediaElement) {
      this.dispatchEvent(new Event("play"));
    },
  );
  const pushed = vi.spyOn(history, "pushState");
  const replaced = vi.spyOn(history, "replaceState");
  const fetch = vi.fn<typeof globalThis.fetch>(async (input) => {
    const path = String(input);
    if (path === "/api/v1/me") return Response.json(account);
    if (
      path === `/api/v1/recordings/${recording.id}/playback-sources` ||
      path.includes("/sources")
    )
      return Response.json([{ id: "owned-source", ready: true }]);
    return new Response(null, { status: 404 });
  });
  vi.stubGlobal("fetch", fetch);
  const recoveries: string[] = [];
  http.setClient({
    async request(options) {
      const url = new URL(String(options.url), window.location.href);
      recoveries.push(url.pathname + url.search);
      return {
        status: 200,
        headers: { "x-inertia": "true", "content-type": "application/json" },
        data: JSON.stringify(
          page(
            url.pathname === "/history" ? "HistoryPage" : "CatalogPage",
            url.pathname + url.search,
          ),
        ),
      };
    },
  });
  document.body.innerHTML =
    '<script data-page="app" type="application/json"></script><div id="app"></div>';
  document.querySelector("script")!.textContent = JSON.stringify(
    page("HistoryPage", "/history"),
  );

  await import("../main");
  await screen.findByRole("heading", { name: "Listening history" });
  await waitFor(() => expect(replaced).toHaveBeenCalled());
  const initial = nativeState.call(history);
  expect(JSON.stringify(initial)).not.toContain(privateTitle);
  expect(JSON.stringify(initial)).not.toContain(privateArtist);
  fireEvent.click(screen.getByRole("button", { name: "Play tracks" }));
  await waitFor(() =>
    expect(document.querySelector("audio")?.getAttribute("src")).toContain(
      "owned-source",
    ),
  );
  const audio = document.querySelector("audio");

  router.push({
    component: "CatalogPage",
    url: "/library",
    props: page("CatalogPage", "/library").props,
  });
  await screen.findByRole("heading", { name: "Your library" });
  expect(document.querySelector("audio")).toBe(audio);
  expect(audio?.getAttribute("src")).toContain("owned-source");
  nativeReplace.call(history, initial, "", "/history");
  window.dispatchEvent(new PopStateEvent("popstate", { state: initial }));
  await screen.findByRole("heading", { name: "Listening history" });
  expect(document.querySelector("audio")).toBe(audio);
  expect(recoveries).toEqual(["/history"]);

  window.scrollTo(0, 120);
  window.dispatchEvent(new Event("scroll"));
  router.remember({ privateTitle }, "owned-private-form");
  await waitFor(() => expect(replaced.mock.calls.length).toBeGreaterThan(1));
  history.replaceState(
    {
      page: page("HistoryPage", "/history"),
      scrollRegions: Array(1100).fill({ top: 5, left: 7, privateTitle }),
      documentScrollPosition: { top: 120, left: 0, privateArtist },
    },
    "",
    "/history",
  );
  expect(nativeState.call(history).scrollRegions).toHaveLength(1000);
  expect(nativeState.call(history).documentScrollPosition).toEqual({
    top: 120,
    left: 0,
  });
  nativeReplace.call(history, null, "", "/library");
  window.dispatchEvent(new PopStateEvent("popstate", { state: null }));
  await waitFor(() => expect(recoveries).toContain("/library"));
  await screen.findByRole("heading", { name: "Your library" });
  expect(document.querySelector("audio")).toBe(audio);

  const beforeSessionClear = nativeState.call(history);
  notifySessionChanged();
  nativeReplace.call(history, beforeSessionClear, "", "/history");
  window.dispatchEvent(
    new PopStateEvent("popstate", { state: beforeSessionClear }),
  );
  await waitFor(() =>
    expect(recoveries.filter((url) => url === "/history")).toHaveLength(2),
  );
  const writes = [...pushed.mock.calls, ...replaced.mock.calls];
  for (const [state] of writes) {
    const persisted = JSON.stringify(state);
    expect(persisted).not.toContain(privateTitle);
    expect(persisted).not.toContain(privateArtist);
    expect(persisted).not.toContain("rememberedState");
    expect(persisted).not.toContain('"props"');
    expect(persisted).not.toContain('"user"');
  }
  expect(JSON.stringify(nativeState.call(history))).not.toContain('"props"');
  expect(JSON.stringify(localStorage)).not.toContain(privateTitle);
  expect(JSON.stringify(sessionStorage)).not.toContain(privateTitle);
});
