import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import Sidebar from "../Sidebar";
import AccountMenu from "../AccountMenu";
import LibrarySections from "../LibrarySections";
import { NoticeProvider } from "../../shared/ui/Notices";
import { usePlayer } from "../../features/playback/components/PlaybackProvider";

vi.mock("@inertiajs/react", () => ({
  router: { clearHistory: vi.fn(), reload: vi.fn() },
  Link: ({ children, href, ...rest }: { children: ReactNode; href: string }) =>
    createElement("a", { href, ...rest }, children),
}));
vi.mock("../../features/playback/components/PlaybackProvider", () => ({
  usePlayer: vi.fn(),
}));
vi.mock("../searchShortcut", () => ({
  openSearch: vi.fn(),
  shortcutLabel: () => ({ text: "⌘K", keys: "Meta+K" }),
  isApplePlatform: () => true,
}));

const user = { id: "alice-id", username: "alice", admin: false };
const lock = { pin_enabled: true, idle_minutes: 0, locked: false };
const playlists = [
  { id: "p1", name: "Sunday calm" },
  { id: "p2", name: "Running" },
];

function sidebar(path: string) {
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(Sidebar, { playlists: playlists as never, path }),
    ),
  );
  return screen.getByRole("navigation", { name: "Main navigation" });
}

function account(path: string, pin = true) {
  vi.mocked(usePlayer).mockReturnValue({ clear: vi.fn() } as never);
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(AccountMenu, {
        user,
        lock: { ...lock, pin_enabled: pin },
        path,
      }),
    ),
  );
}

test("the main navigation offers Home and Library links and a Search button", () => {
  const navigation = sidebar("/library");
  const links = within(navigation).getAllByRole("link");
  expect(links.map((link) => link.textContent)).toEqual(["Home", "Library"]);
  expect(links.map((link) => link.getAttribute("href"))).toEqual([
    "/discover",
    "/library",
  ]);
  const search = within(navigation).getByRole("button", {
    name: "Open search",
  });
  expect(search.textContent).toContain("Search");
  expect(search.getAttribute("aria-keyshortcuts")).toBe("Meta+K");
  expect(search.querySelector("kbd")?.getAttribute("aria-hidden")).toBe("true");
  expect(search.querySelector("kbd")?.textContent).toBe("⌘K");
});

test.each([
  ["/library", "Library"],
  ["/", "Library"],
  ["/albums", "Library"],
  ["/artists", "Library"],
  ["/artist-groups/g1", "Library"],
  ["/playlists/p1", "Library"],
  ["/podcasts", "Library"],
  ["/audiobooks", "Library"],
  ["/likes", "Library"],
  ["/history", "Library"],
  ["/recordings/r1", "Library"],
  ["/discover", "Home"],
])("%s marks %s as the current destination", (path, current) => {
  const navigation = sidebar(path);
  const marked = within(navigation)
    .getAllByRole("link")
    .filter((link) => link.getAttribute("aria-current") === "page");
  expect(marked.map((link) => link.textContent)).toEqual([current]);
});

test("the Search button asks the shell to open search", async () => {
  const { openSearch } = await import("../searchShortcut");
  const navigation = sidebar("/library");
  fireEvent.click(
    within(navigation).getByRole("button", { name: "Open search" }),
  );
  expect(openSearch).toHaveBeenCalled();
});

test("the mark names the product", () => {
  sidebar("/library");
  expect(screen.getByRole("link", { name: /hito/ })).toBeTruthy();
});

test("playlists are listed with a create link", () => {
  sidebar("/library");
  const list = document.querySelector(".sidebar-playlists") as HTMLElement;
  expect(
    within(list)
      .getByRole("link", { name: "Create a playlist" })
      .getAttribute("href"),
  ).toBe("/playlists#new-playlist");
  expect(
    within(list)
      .getByRole("link", { name: "Sunday calm" })
      .getAttribute("href"),
  ).toBe("/playlists/p1");
});

test("the avatar opens account and settings while lock and sign out stay reachable", () => {
  account("/library");
  expect(
    screen
      .getByRole("link", { name: "Account and settings" })
      .getAttribute("href"),
  ).toBe("/settings");
  expect(screen.getByRole("button", { name: "Lock" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Sign out" })).toBeTruthy();
});

test("lock is offered only with a PIN", () => {
  account("/library", false);
  expect(screen.queryByRole("button", { name: "Lock" })).toBeNull();
});

test("the settings link is current on settings pages", () => {
  account("/settings");
  expect(
    screen
      .getByRole("link", { name: "Account and settings" })
      .getAttribute("aria-current"),
  ).toBe("page");
});

test("library sections keep every collection page reachable from Library", () => {
  render(createElement(LibrarySections, { path: "/artist-groups/g1" }));
  const navigation = screen.getByRole("navigation", {
    name: "Library sections",
  });
  const links = within(navigation).getAllByRole("link");
  expect(links.map((link) => link.getAttribute("href"))).toEqual([
    "/library",
    "/albums",
    "/artists",
    "/playlists",
    "/podcasts",
    "/audiobooks",
    "/likes",
    "/history",
  ]);
  expect(
    links
      .filter((link) => link.getAttribute("aria-current") === "page")
      .map((link) => link.textContent),
  ).toEqual(["Artists"]);
});

test("library sections are absent outside the library family", () => {
  const view = render(createElement(LibrarySections, { path: "/settings" }));
  expect(view.container.firstChild).toBeNull();
});
