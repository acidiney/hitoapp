import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import AlbumGrid from "../components/AlbumGrid";

vi.mock("@inertiajs/react", () => ({
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

const release = {
  id: "release-1",
  title: "Costa Norte",
  artist: "Lume Areia",
  artist_id: "artist-1",
  track_count: 4,
  duration_ms: 1000,
} as unknown as Schema<"Release">;

test("an album without artwork gets a deterministic generated cover", () => {
  const { container } = render(
    createElement(AlbumGrid, { releases: [release] }),
  );
  fireEvent.error(container.querySelector("img")!);
  expect(
    screen.getByRole("img", { name: "Cover of Costa Norte", hidden: true }),
  ).toBeTruthy();
});

test("a card without an album artist shows the server's inferred credit as plain text", () => {
  const unnamed = { ...release, artist: null, artist_id: null };
  const named = {
    ...unnamed,
    id: "r2",
    inferred_artist: { kind: "named", name: "Lume Areia" },
  } as unknown as Schema<"Release">;
  const various = {
    ...unnamed,
    id: "r3",
    inferred_artist: { kind: "various" },
  } as unknown as Schema<"Release">;
  const none = {
    ...unnamed,
    id: "r4",
    inferred_artist: null,
  } as unknown as Schema<"Release">;
  render(createElement(AlbumGrid, { releases: [named, various, none] }));
  expect(screen.getByText("Lume Areia").closest("a")).toBeNull();
  expect(screen.getByText("Various artists").closest("a")).toBeNull();
  expect(screen.getAllByText(/^4 tracks$/)).toHaveLength(3);
});
