import { createElement, type ReactNode } from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import HomeShelf from "../components/HomeShelf";
import { artistShelf } from "../model/homeShelves";

vi.mock("@inertiajs/react", () => ({
  Link: ({ href, children, ...rest }: { href: string; children: ReactNode }) =>
    createElement("a", { href, ...rest }, children),
}));
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("the artist shelf draws the artist cover as a round photo", () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response("{}", { status: 404 })),
  );
  const cards = artistShelf([
    {
      id: "g1",
      provider: { kind: "builtin_musicbrainz" },
      provider_artist_id: "mb",
      name: {
        artist_id: "a",
        association_id: "b",
        recording_id: "r1",
        value: "Slipknot",
      },
      library_count: 1,
      recording_count: 3,
      release_count: 1,
    },
  ]);
  const { container } = render(
    createElement(ArtworkProvider, {
      account: "alice",
      children: createElement(HomeShelf, {
        id: "artists",
        title: "Your artists",
        cards,
      }),
    }),
  );
  const cover = container.querySelector(
    ".shelf-card[data-round] .fallback-cover",
  );
  expect(cover).not.toBeNull();
  expect(cover!.getAttribute("data-kind")).toBe("artist");
  expect(cover!.getAttribute("aria-label")).toBe("Photo of Slipknot");
});
