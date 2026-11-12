import { createElement } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import AlbumActions, { type AlbumPlay } from "../components/AlbumActions";

const router = vi.hoisted(() => ({ visit: vi.fn() }));
vi.mock("@inertiajs/react", () => ({ router }));

function viewport(phone: boolean) {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: phone && query.includes("767px"),
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
}

function play(): AlbumPlay {
  return { disabled: false, play: vi.fn(), shuffle: vi.fn() };
}

function menuItems() {
  fireEvent.click(screen.getByRole("button", { name: "More actions" }));
  return screen.getAllByRole("menuitem").map((item) => item.textContent);
}

test("an owned album on a wide screen shows Play, a Shuffle icon and More with edit details", () => {
  viewport(false);
  const actions = play();
  render(
    createElement(AlbumActions, {
      play: actions,
      detailsHref: "/albums/album%201/details",
      retry: null,
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Play album" }));
  fireEvent.click(screen.getByRole("button", { name: "Shuffle album" }));
  expect(actions.play).toHaveBeenCalledOnce();
  expect(actions.shuffle).toHaveBeenCalledOnce();
  expect(screen.queryByRole("link", { name: "Edit album details" })).toBeNull();
  expect(menuItems()).toEqual(["Edit album details"]);
  fireEvent.click(screen.getByRole("menuitem", { name: "Edit album details" }));
  expect(router.visit).toHaveBeenCalledWith("/albums/album%201/details");
});

test("an owned album on a phone keeps Shuffle, edit details and artwork retry in More", () => {
  viewport(true);
  const actions = play();
  const retry = { label: "Retry artwork", onSelect: vi.fn() };
  render(
    createElement(AlbumActions, {
      play: actions,
      detailsHref: "/albums/a/details",
      retry,
    }),
  );
  expect(screen.queryByRole("button", { name: "Shuffle album" })).toBeNull();
  expect(menuItems()).toEqual([
    "Shuffle album",
    "Edit album details",
    "Retry artwork",
  ]);
  fireEvent.click(screen.getByRole("menuitem", { name: "Shuffle album" }));
  expect(actions.shuffle).toHaveBeenCalledOnce();
});

test("an album you do not own on a wide screen has no More menu and no edit details", () => {
  viewport(false);
  render(
    createElement(AlbumActions, {
      play: play(),
      detailsHref: null,
      retry: null,
    }),
  );
  expect(screen.getByRole("button", { name: "Shuffle album" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "More actions" })).toBeNull();
});

test("an album you do not own on a phone offers only Shuffle in More", () => {
  viewport(true);
  render(
    createElement(AlbumActions, {
      play: play(),
      detailsHref: null,
      retry: null,
    }),
  );
  expect(menuItems()).toEqual(["Shuffle album"]);
});

test("an empty album disables Play and Shuffle", () => {
  viewport(false);
  render(
    createElement(AlbumActions, {
      play: { ...play(), disabled: true },
      detailsHref: null,
      retry: null,
    }),
  );
  expect(
    (screen.getByRole("button", { name: "Play album" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  expect(
    (screen.getByRole("button", { name: "Shuffle album" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
});
