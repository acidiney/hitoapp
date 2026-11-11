import { createElement } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import AlbumHeaderActions from "../components/AlbumHeaderActions";

const router = vi.hoisted(() => ({ visit: vi.fn() }));
const artwork = vi.hoisted(() => ({
  state: { imageFailed: false, phase: "ready", retry: vi.fn() },
}));
vi.mock("@inertiajs/react", () => ({
  router,
  Link: ({
    children,
    ...props
  }: {
    children: string;
    href: string;
    className: string;
  }) => createElement("a", props, children),
}));
vi.mock("../../artwork/components/ArtworkProvider", () => ({
  useArtwork: () => artwork.state,
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({ playQueue: vi.fn() }),
}));

function viewport(phone: boolean) {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: phone && query.includes("767px"),
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
}

function actions() {
  return createElement(AlbumHeaderActions, {
    release: "album 1",
    tracks: [],
    listing: undefined as never,
  });
}

test("on a phone, edit details and retry artwork live in one More actions menu", () => {
  viewport(true);
  artwork.state = { imageFailed: true, phase: "unavailable", retry: vi.fn() };
  render(actions());
  expect(screen.queryByRole("link", { name: "Edit album details" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "More actions" }));
  expect(screen.getByRole("menuitem", { name: "Retry artwork" })).toBeTruthy();
  fireEvent.click(screen.getByRole("menuitem", { name: "Edit album details" }));
  expect(router.visit).toHaveBeenCalledWith("/albums/album%201/details");
});

test("on a phone, a working cover leaves shuffle and edit in the menu", () => {
  viewport(true);
  artwork.state = { imageFailed: false, phase: "ready", retry: vi.fn() };
  render(actions());
  fireEvent.click(screen.getByRole("button", { name: "More actions" }));
  expect(
    screen.getAllByRole("menuitem").map((item) => item.textContent),
  ).toEqual(["Shuffle album", "Edit album details"]);
});

test("on a wide screen, edit details stays a link in the header", () => {
  viewport(false);
  artwork.state = { imageFailed: false, phase: "ready", retry: vi.fn() };
  render(actions());
  expect(
    screen
      .getByRole("link", { name: "Edit album details" })
      .getAttribute("href"),
  ).toBe("/albums/album%201/details");
  expect(screen.queryByRole("button", { name: "More actions" })).toBeNull();
});
