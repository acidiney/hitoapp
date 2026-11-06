import { createElement, type AnchorHTMLAttributes } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
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
  foundRecording,
  twoEditionsPage,
} from "../fixtures/catalogAlbumPlay";
import { router } from "@inertiajs/react";
import { choose } from "../../../test/listbox";

const player = vi.hoisted(() => {
  const selected = vi.fn(async (_target: unknown) => true);
  return {
    selected,
    prepareSelection: vi.fn(() => selected),
    playQueue: vi.fn(),
    shuffle: vi.fn(),
    subscribe: () => () => {},
    snapshot: () => ({ current: null, shuffle: false }),
    queueSnapshot: () => ({
      version: 0,
      total: 1,
      currentIndex: 0 as number | null,
      currentEntry: null,
      pending: 0,
    }),
  };
});
const mutate = vi.hoisted(() =>
  vi.fn(async (_action: unknown) => ({ ok: true })),
);
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate, pending: false }),
}));
const location = vi.hoisted(() => ({ url: "/" }));
vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn(), reload: vi.fn(), visit: vi.fn() },
  usePage: () => ({ url: location.url }),
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

const base = `/api/v1/recordings/${artistAssociation.recording_id}/artist-associations`;
const missing = new Set<string>();
const audioBodies: Array<{ occurrence_id: string; edition_id: string }> = [];
const trackBodies: Array<{ edition_id: string }> = [];

function serve() {
  const fetch = vi.fn(async (path: string, init?: RequestInit) => {
    const body = init?.body ? JSON.parse(init.body as string) : null;
    if (path === base) return Response.json([artistAssociation]);
    if (path === `${base}/${artistAssociation.id}/profile`)
      return Response.json(reviewedArtistProfile);
    if (path === `${base}/${artistAssociation.id}/discography/library`)
      return Response.json({
        association_id: artistAssociation.id,
        groups: body.release_group_ids.map((id: string) => ({
          release_group_id: id,
          albums: [],
        })),
      });
    if (path === `${base}/${artistAssociation.id}/release-group-editions`)
      return Response.json(twoEditionsPage);
    if (path === `${base}/${artistAssociation.id}/edition-track-list`) {
      trackBodies.push(body);
      return Response.json({
        ...catalogAlbumTrackList,
        edition: { ...catalogAlbumEdition, id: body.edition_id },
      });
    }
    if (path === `${base}/${artistAssociation.id}/edition-track-audio`) {
      audioBodies.push(body);
      const index = Number(body.occurrence_id.split("/").pop()) - 1;
      return Response.json(
        missing.has(body.occurrence_id)
          ? { status: "no_match" }
          : { status: "saved", recording: foundRecording(index) },
      );
    }
    throw new Error(`Unexpected request ${path}`);
  });
  vi.stubGlobal("fetch", fetch);
  return fetch;
}

beforeEach(() => {
  const { detail } = artistIdentityBrowsing();
  location.url = `/artist-groups/${detail.detail!.group.id}/releases/${encodeURIComponent(editionGroup.id)}`;
});

afterEach(() => {
  cleanup();
  missing.clear();
  audioBodies.length = 0;
  trackBodies.length = 0;
  mutate.mockClear();
  player.selected.mockClear();
  player.prepareSelection.mockClear();
});

async function openPage() {
  serve();
  render(createElement(CatalogAlbumPage, artistIdentityBrowsing().detail));
  const table = await screen.findByRole("table", { name: "Tracks" });
  await within(table).findByText(catalogAlbumTitles[0]);
  return table;
}

test("a catalog release opens as its own page with the ordered tracks in standard rows", async () => {
  const table = await openPage();
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBeTruthy();
  const rows = within(table).getAllByRole("row").slice(1);
  expect(
    rows.map((row) =>
      within(row)
        .getByRole("button", { name: /^Play / })
        .getAttribute("aria-label"),
    ),
  ).toEqual(catalogAlbumTitles.map((title) => `Play ${title}`));
  const { group } = artistIdentityBrowsing().detail.detail!;
  expect(
    within(screen.getByRole("navigation", { name: "Breadcrumb" }))
      .getByRole("link", { name: group.name.value })
      .getAttribute("href"),
  ).toBe(`/artist-groups/${group.id}`);
});

test("playing a catalog row selects through the shared player queue", async () => {
  const table = await openPage();
  fireEvent.click(
    within(table).getByRole("button", {
      name: `Play ${catalogAlbumTitles[1]}`,
    }),
  );
  await waitFor(() => expect(player.selected).toHaveBeenCalledTimes(1));
  expect(player.prepareSelection).toHaveBeenCalledTimes(1);
  const target = player.selected.mock.calls[0][0] as {
    queue: Array<{ id: string }>;
    index: number;
  };
  expect(target.queue.map((track) => track.id)).toEqual(["recording-found-2"]);
  expect(target.index).toBe(0);
  expect(audioBodies[0].occurrence_id.endsWith("/2")).toBe(true);
});

test("a row without audio can find it and then offers like and details like a library row", async () => {
  const table = await openPage();
  const title = catalogAlbumTitles[0];
  expect(
    within(table).queryByRole("button", { name: `Like ${title}` }),
  ).toBeNull();
  fireEvent.click(
    within(table).getByRole("button", { name: `Find audio for ${title}` }),
  );
  const details = await within(table).findByRole("link", { name: title });
  expect(details.getAttribute("href")).toBe("/recordings/recording-found-1");
  fireEvent.click(within(table).getByRole("button", { name: `Like ${title}` }));
  await waitFor(() =>
    expect(mutate).toHaveBeenCalledWith({
      kind: "like_set",
      recording_id: "recording-found-1",
      liked: true,
    }),
  );
  expect(
    await within(table).findByRole("button", { name: `Unlike ${title}` }),
  ).toBeTruthy();
  expect(player.selected).not.toHaveBeenCalled();
});

test("a track with no confident match says so and lets the owner retry or connect services", async () => {
  const table = await openPage();
  const title = catalogAlbumTitles[2];
  missing.add("opaque:occurrence/play/3");
  fireEvent.click(
    within(table).getByRole("button", { name: `Find audio for ${title}` }),
  );
  expect(
    await within(table).findByText("Not found on your services"),
  ).toBeTruthy();
  expect(
    within(table).getByRole("button", {
      name: `Try again to find audio for ${title}`,
    }),
  ).toBeTruthy();
  fireEvent.click(
    within(table).getByRole("button", { name: `Actions for ${title}` }),
  );
  fireEvent.click(screen.getByRole("menuitem", { name: "Connect services" }));
  expect(router.visit).toHaveBeenCalledWith("/settings?section=music");
});

test("the edition switcher is a listbox in the header and loads that edition's tracks", async () => {
  await openPage();
  const header = screen.getByRole("banner");
  const control = within(header).getByRole("combobox", { name: "Edition" });
  expect(document.querySelector("select[aria-label='Edition']")).toBeNull();
  choose(control, 1);
  await waitFor(() =>
    expect(trackBodies.map((body) => body.edition_id)).toContain(
      twoEditionsPage.page.items[1].id,
    ),
  );
});

test("an unfound row has details and like buttons that find audio first", async () => {
  const table = await openPage();
  const title = catalogAlbumTitles[3];
  fireEvent.click(
    within(table).getByRole("button", { name: `Find audio and like ${title}` }),
  );
  await waitFor(() =>
    expect(mutate).toHaveBeenCalledWith({
      kind: "like_set",
      recording_id: "recording-found-4",
      liked: true,
    }),
  );
  expect(
    await within(table).findByRole("button", { name: `Unlike ${title}` }),
  ).toBeTruthy();
  const other = catalogAlbumTitles[0];
  fireEvent.click(
    within(table).getByRole("button", { name: `Actions for ${other}` }),
  );
  fireEvent.click(screen.getByRole("menuitem", { name: "Track details" }));
  await waitFor(() =>
    expect(router.get).toHaveBeenCalledWith("/recordings/recording-found-1"),
  );
});
