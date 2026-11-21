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

function addition(
  missing: number,
  total: number,
  extra: Partial<import("../hooks/useAlbumAddition").AlbumAdditionState> = {},
) {
  return {
    missing,
    total,
    phase: null,
    added: null,
    undone: null,
    error: "",
    add: vi.fn().mockResolvedValue(undefined),
    undo: vi.fn().mockResolvedValue(undefined),
    ...extra,
  };
}

test("an album you own none of offers Add album, and a partly owned one offers the rest", () => {
  viewport(false);
  const none = addition(4, 4);
  const view = render(
    createElement(AlbumActions, {
      play: play(),
      addition: none,
      detailsHref: null,
      retry: null,
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Add album" }));
  expect(none.add).toHaveBeenCalledOnce();
  view.rerender(
    createElement(AlbumActions, {
      play: play(),
      addition: addition(2, 5),
      detailsHref: null,
      retry: null,
    }),
  );
  expect(screen.getByRole("button", { name: "Add the other 2" })).toBeTruthy();
  view.rerender(
    createElement(AlbumActions, {
      play: play(),
      addition: addition(0, 5),
      detailsHref: null,
      retry: null,
    }),
  );
  expect(screen.queryByRole("button", { name: /^Add/ })).toBeNull();
});

test("on a phone the add sits in More, next to Shuffle", () => {
  viewport(true);
  render(
    createElement(AlbumActions, {
      play: play(),
      addition: addition(3, 3),
      detailsHref: null,
      retry: null,
    }),
  );
  expect(menuItems()).toEqual(["Shuffle album", "Add album"]);
});

test("after adding, the status says how many and offers Undo; after undo it says what stayed", () => {
  viewport(false);
  const added = addition(0, 4, {
    added: { id: "addition-1", release_id: "release-1", added: 4 },
  });
  const view = render(
    createElement(AlbumActions, {
      play: play(),
      addition: added,
      detailsHref: null,
      retry: null,
    }),
  );
  expect(screen.getByRole("status").textContent).toContain(
    "Added 4 tracks to your library.",
  );
  fireEvent.click(screen.getByRole("button", { name: "Undo" }));
  expect(added.undo).toHaveBeenCalledOnce();
  view.rerender(
    createElement(AlbumActions, {
      play: play(),
      addition: addition(4, 4, { undone: { removed: 3, kept: 1 } }),
      detailsHref: null,
      retry: null,
    }),
  );
  expect(screen.getByRole("status").textContent).toBe(
    "Removed 3 tracks. Kept 1 track you've already used.",
  );
});
