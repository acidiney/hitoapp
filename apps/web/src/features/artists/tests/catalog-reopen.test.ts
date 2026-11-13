import { createElement, type AnchorHTMLAttributes } from "react";
import {
  cleanup,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import CatalogAlbumPage from "../pages/CatalogAlbumPage";
import { artistIdentityBrowsing } from "../fixtures/artistIdentityBrowsing";
import { editionGroup } from "../fixtures/artistEditions";
import {
  artistAssociation,
  reviewedArtistProfile,
} from "../fixtures/artistReview";
import {
  catalogAlbumEdition,
  catalogAlbumTitles,
  catalogAlbumTrackList,
  twoEditionsPage,
} from "../fixtures/catalogAlbumPlay";

vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({
    selected: vi.fn(),
    prepareSelection: vi.fn(),
    playQueue: vi.fn(),
    subscribe: () => () => {},
    snapshot: () => ({ current: null, shuffle: false }),
    queueSnapshot: () => ({ total: 1, currentIndex: 0 }),
  }),
}));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn(), reload: vi.fn() },
  usePage: () => ({
    url: `/artist-groups/g/releases/${encodeURIComponent(editionGroup.id)}`,
  }),
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

afterEach(cleanup);

test("opening the same catalog album again reuses the provider answers", async () => {
  const base = `/api/v1/recordings/${artistAssociation.recording_id}/artist-associations`;
  const counts: Record<string, number> = {};
  const fetch = vi.fn(async (path: string, init?: RequestInit) => {
    const key = path.replace(base, "");
    counts[key] = (counts[key] ?? 0) + 1;
    const body = init?.body ? JSON.parse(init.body as string) : null;
    if (path === base) return Response.json([artistAssociation]);
    if (path.endsWith("/profile")) return Response.json(reviewedArtistProfile);
    if (path.endsWith("/discography/library"))
      return Response.json({
        association_id: artistAssociation.id,
        groups: body.release_group_ids.map((id: string) => ({
          release_group_id: id,
          albums: [],
        })),
      });
    if (path.endsWith("/release-group-editions"))
      return Response.json(twoEditionsPage);
    if (path.endsWith("/edition-track-list"))
      return Response.json({
        ...catalogAlbumTrackList,
        edition: { ...catalogAlbumEdition, id: body.edition_id },
      });
    throw new Error(`Unexpected request ${path}`);
  });
  vi.stubGlobal("fetch", fetch);
  const detail = artistIdentityBrowsing().detail;
  for (let visit = 0; visit < 2; visit++) {
    const view = render(createElement(CatalogAlbumPage, detail));
    const table = await screen.findByRole("list", { name: "Tracks" });
    await within(table).findByText(catalogAlbumTitles[0]);
    view.unmount();
  }
  await waitFor(() => expect(fetch).toHaveBeenCalled());
  expect(counts["/" + artistAssociation.id + "/profile"]).toBe(1);
  expect(counts["/" + artistAssociation.id + "/release-group-editions"]).toBe(
    1,
  );
  expect(counts["/" + artistAssociation.id + "/edition-track-list"]).toBe(1);
});
