import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import Sidebar from "../Sidebar";
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

const user = { id: "alice-id", username: "alice", admin: false };
const lock = { pin_enabled: true, idle_minutes: 0, locked: false };
const playlists = [
  { id: "p1", name: "Sunday calm" },
  { id: "p2", name: "Running" },
];

function sidebar(path: string) {
  vi.mocked(usePlayer).mockReturnValue({ clear: vi.fn() } as never);
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(Sidebar, {
        user,
        playlists: playlists as never,
        lock,
        path,
      }),
    ),
  );
  return screen.getByRole("navigation", { name: "Main navigation" });
}

test("the main navigation has exactly three destinations with their routes", () => {
  const navigation = sidebar("/library");
  const links = within(navigation).getAllByRole("link");
  expect(links.map((link) => link.textContent)).toEqual([
    "Início",
    expect.stringContaining("Pesquisar"),
    "Biblioteca",
  ]);
  expect(links.map((link) => link.getAttribute("href"))).toEqual([
    "/discover",
    "/library",
    "/library",
  ]);
});

test.each([
  ["/library", "Biblioteca"],
  ["/", "Biblioteca"],
  ["/albums", "Biblioteca"],
  ["/artists", "Biblioteca"],
  ["/artist-groups/g1", "Biblioteca"],
  ["/playlists/p1", "Biblioteca"],
  ["/podcasts", "Biblioteca"],
  ["/audiobooks", "Biblioteca"],
  ["/likes", "Biblioteca"],
  ["/history", "Biblioteca"],
  ["/recordings/r1", "Biblioteca"],
  ["/discover", "Início"],
])("%s marks %s as the current destination", (path, current) => {
  const navigation = sidebar(path);
  const marked = within(navigation)
    .getAllByRole("link")
    .filter((link) => link.getAttribute("aria-current") === "page");
  expect(marked.map((link) => link.textContent)).toEqual([current]);
});

test("search shows the keyboard hint and the mark names the product", () => {
  const navigation = sidebar("/library");
  expect(within(navigation).getByText("⌘K")).toBeTruthy();
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
  sidebar("/library");
  expect(
    screen
      .getByRole("link", { name: "Conta e definições" })
      .getAttribute("href"),
  ).toBe("/settings");
  expect(screen.getByRole("button", { name: "Lock" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Sign out" })).toBeTruthy();
});

test("the settings link is current on settings pages", () => {
  sidebar("/settings");
  expect(
    screen
      .getByRole("link", { name: "Conta e definições" })
      .getAttribute("aria-current"),
  ).toBe("page");
});

test("search focuses the search field on pages that have one", () => {
  const input = document.createElement("input");
  input.id = "shell-search";
  document.body.append(input);
  const navigation = sidebar("/library");
  fireEvent.click(within(navigation).getByRole("link", { name: /Pesquisar/ }));
  expect(document.activeElement).toBe(input);
  input.remove();
});

test("library sections keep every collection page reachable from Biblioteca", () => {
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
