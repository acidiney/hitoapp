import type { Schema } from "../../../shared/schema";
import { createElement, useState, type ReactNode } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import AlbumIndexPage from "../pages/AlbumIndexPage";

vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn() },
  useRemember: (initial: unknown) => useState(initial),
  Link: ({ children, href, ...rest }: { children: ReactNode; href: string }) =>
    createElement("a", { href, ...rest }, children),
}));
vi.mock("../../artwork/components/ArtworkProvider", () => ({
  useArtwork: () => ({
    generation: 0,
    revision: 0,
    loaded: () => {},
    missing: () => {},
  }),
}));

const release = (id: string) =>
  ({
    id,
    title: `Album ${id}`,
    artist: "Lume Areia",
    artist_id: "artist-1",
    track_count: 4,
    duration_ms: 1000,
  }) as unknown as Schema<"Release">;

function props(): Schema<"AlbumIndexPageProps"> {
  return {
    user: { id: "alice" },
    query: "Album",
    releases: {
      items: [release("a"), release("b")],
      total: 3,
      offset: 0,
      limit: 2,
    },
  } as unknown as Schema<"AlbumIndexPageProps">;
}

test("the albums grid appends the next page when its sentinel scrolls into view and has no pager", async () => {
  let fire = () => {};
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      constructor(private callback: IntersectionObserverCallback) {}
      observe() {
        fire = () =>
          this.callback(
            [{ isIntersecting: true } as IntersectionObserverEntry],
            this as unknown as IntersectionObserver,
          );
      }
      disconnect() {}
    },
  );
  const fetch = vi.fn(async (_path: string) =>
    Response.json({ items: [release("c")], total: 3, offset: 2, limit: 2 }),
  );
  vi.stubGlobal("fetch", fetch);
  render(createElement(AlbumIndexPage, props()));
  expect(screen.queryByRole("button", { name: "Next" })).toBeNull();
  expect(screen.getByText("Showing 2 of 3 albums")).toBeTruthy();
  act(() => fire());
  await waitFor(() =>
    expect(screen.getByRole("link", { name: "Album c" })).toBeTruthy(),
  );
  expect(fetch.mock.calls[0][0]).toBe("/api/v1/releases?q=Album&offset=2");
  expect(screen.queryByRole("button", { name: "Load more" })).toBeNull();
});

test("Load more is the button fallback when the browser gives no scroll signal", async () => {
  const fetch = vi.fn(async () =>
    Response.json({ items: [release("c")], total: 3, offset: 2, limit: 2 }),
  );
  vi.stubGlobal("fetch", fetch);
  render(createElement(AlbumIndexPage, props()));
  fireEvent.click(screen.getByRole("button", { name: "Load more" }));
  await waitFor(() =>
    expect(screen.getByRole("link", { name: "Album c" })).toBeTruthy(),
  );
});
