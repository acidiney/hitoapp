import { chooseEdition } from "../../catalog/fixtures/editionMenu";
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
import { forgetCatalog } from "../api/catalogCache";

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
let profileFails = false;
const audioBodies: Array<{ occurrence_id: string; edition_id: string }> = [];
const trackBodies: Array<{ edition_id: string }> = [];
let linked: Array<{
  occurrence_id: string;
  recording: ReturnType<typeof foundRecording>;
}> = [];

function serve() {
  const fetch = vi.fn(async (path: string, init?: RequestInit) => {
    const body = init?.body ? JSON.parse(init.body as string) : null;
    if (path === base) return Response.json([artistAssociation]);
    if (path === `${base}/${artistAssociation.id}/profile`)
      return profileFails
        ? Response.json({ error: "Profile unavailable" }, { status: 500 })
        : Response.json(reviewedArtistProfile);
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
        linked,
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
  forgetCatalog();
  linked = [];
  const { detail } = artistIdentityBrowsing();
  location.url = `/artist-groups/${detail.detail!.group.id}/releases/${encodeURIComponent(editionGroup.id)}`;
});

afterEach(() => {
  cleanup();
  missing.clear();
  profileFails = false;
  audioBodies.length = 0;
  trackBodies.length = 0;
  mutate.mockClear();
  player.selected.mockClear();
  player.prepareSelection.mockClear();
});

async function openPage() {
  serve();
  render(
    createElement(CatalogAlbumPage, {
      ...artistIdentityBrowsing().detail,
      release_group_id: editionGroup.id,
    }),
  );
  const list = await screen.findByRole("list", { name: "Tracks" });
  await within(list).findByText(catalogAlbumTitles[0]);
  return list;
}

function rowOf(list: HTMLElement, title: string) {
  const row = within(list)
    .getAllByRole("listitem")
    .find(
      (item) => item.querySelector(".edition-row-title")?.textContent === title,
    );
  if (!row) throw new Error(`No row ${title}`);
  return row;
}

function openRowMenu(row: HTMLElement, title: string) {
  fireEvent.click(
    within(row).getByRole("button", {
      name: new RegExp(`^(More actions|Actions) for ${title}$`),
    }),
  );
}

test("a catalog release opens as its own page with the ordered tracks in edition rows that always name the credit", async () => {
  const list = await openPage();
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBeTruthy();
  expect(screen.queryByRole("table")).toBeNull();
  const rows = within(list).getAllByRole("listitem");
  expect(
    rows.map((row) =>
      within(row)
        .getByRole("button", { name: /^Play / })
        .getAttribute("aria-label"),
    ),
  ).toEqual(catalogAlbumTitles.map((title) => `Play ${title}`));
  for (const row of rows) {
    expect(row.querySelector(".edition-row-credit")?.textContent).toBeTruthy();
    expect(row.textContent).toContain("Not in your library");
    expect(
      within(row).getByRole("button", { name: /^Add .* to your library$/ }),
    ).toBeTruthy();
    expect(
      within(row).queryByRole("img", { name: "In your library" }),
    ).toBeNull();
  }
  expect(screen.queryByRole("button", { name: /^In my library/ })).toBeNull();
  const header = document.querySelector<HTMLElement>(".album-header")!;
  expect(within(header).getByText("Not in your library")).toBeTruthy();
  expect(
    within(header).queryByRole("img", { name: /tracks in your library/ }),
  ).toBeNull();
});

test("playing a catalog row selects through the shared player queue", async () => {
  const list = await openPage();
  fireEvent.click(
    within(list).getByRole("button", {
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

test("adding a row finds its audio, marks it in your library with the check and enables the library filter", async () => {
  const list = await openPage();
  const title = catalogAlbumTitles[0];
  fireEvent.click(
    within(rowOf(list, title)).getByRole("button", {
      name: `Add ${title} to your library`,
    }),
  );
  await within(rowOf(list, title)).findByRole("img", {
    name: "In your library",
  });
  expect(
    within(rowOf(list, title)).queryByRole("button", {
      name: `Add ${title} to your library`,
    }),
  ).toBeNull();
  const header = document.querySelector<HTMLElement>(".album-header")!;
  expect(within(header).getByText("1 of 4 in your library")).toBeTruthy();
  expect(
    within(header).getByRole("img", { name: "1 of 4 tracks in your library" }),
  ).toBeTruthy();
  expect(within(header).queryByText("Not in your library")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "In my library 1" }));
  expect(within(list).getAllByRole("listitem")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "All 4" }));
  expect(within(list).getAllByRole("listitem")).toHaveLength(4);
  openRowMenu(rowOf(list, title), title);
  fireEvent.click(screen.getByRole("menuitem", { name: "Track details" }));
  expect(router.visit).toHaveBeenCalledWith("/recordings/recording-found-1");
  expect(player.selected).not.toHaveBeenCalled();
});

test("a track with no confident match says so and lets the owner retry or connect services", async () => {
  const list = await openPage();
  const title = catalogAlbumTitles[2];
  missing.add("opaque:occurrence/play/3");
  fireEvent.click(
    within(rowOf(list, title)).getByRole("button", {
      name: `Add ${title} to your library`,
    }),
  );
  await waitFor(() =>
    expect(rowOf(list, title).textContent).toContain("No audio found"),
  );
  expect(
    within(rowOf(list, title)).getByRole("button", {
      name: `Add ${title} to your library`,
    }),
  ).toBeTruthy();
  openRowMenu(rowOf(list, title), title);
  fireEvent.click(screen.getByRole("menuitem", { name: "Connect services" }));
  expect(router.visit).toHaveBeenCalledWith("/settings?section=music");
});

test("the edition chip menu above the tracks names the catalog and loads the chosen edition's tracks", async () => {
  await openPage();
  expect(screen.getByText(/^Track list from /)).toBeTruthy();
  chooseEdition(1);
  await waitFor(() =>
    expect(trackBodies.map((body) => body.edition_id)).toContain(
      twoEditionsPage.page.items[1].id,
    ),
  );
});

test("an unfound row's menu finds audio before liking it or opening its details", async () => {
  const list = await openPage();
  const title = catalogAlbumTitles[3];
  openRowMenu(rowOf(list, title), title);
  fireEvent.click(
    screen.getByRole("menuitem", { name: "Find audio and like" }),
  );
  await waitFor(() =>
    expect(mutate).toHaveBeenCalledWith({
      kind: "like_set",
      recording_id: "recording-found-4",
      liked: true,
    }),
  );
  await within(rowOf(list, title)).findByRole("img", {
    name: "In your library",
  });
  const other = catalogAlbumTitles[0];
  openRowMenu(rowOf(list, other), other);
  fireEvent.click(screen.getByRole("menuitem", { name: "Track details" }));
  await waitFor(() =>
    expect(router.get).toHaveBeenCalledWith("/recordings/recording-found-1"),
  );
});

test("a row linked to a saved recording shows as in your library and plays without a new search", async () => {
  const title = catalogAlbumTitles[1];
  linked = [
    { occurrence_id: "opaque:occurrence/play/2", recording: foundRecording(1) },
  ];
  const list = await openPage();
  await within(rowOf(list, title)).findByRole("img", {
    name: "In your library",
  });
  expect(
    within(rowOf(list, title)).queryByRole("button", {
      name: `Add ${title} to your library`,
    }),
  ).toBeNull();
  expect(
    within(list).getByRole("button", {
      name: `Add ${catalogAlbumTitles[0]} to your library`,
    }),
  ).toBeTruthy();
  fireEvent.click(within(list).getByRole("button", { name: `Play ${title}` }));
  await waitFor(() => expect(player.selected).toHaveBeenCalledTimes(1));
  const target = player.selected.mock.calls[0][0] as {
    queue: Array<{ id: string }>;
  };
  expect(target.queue.map((track) => track.id)).toEqual(["recording-found-2"]);
  expect(audioBodies).toHaveLength(0);
});

test("finding audio makes the next visit read the track list again so the new link shows", async () => {
  const list = await openPage();
  const title = catalogAlbumTitles[0];
  fireEvent.click(
    within(list).getByRole("button", {
      name: `Add ${title} to your library`,
    }),
  );
  await within(rowOf(list, title)).findByRole("img", {
    name: "In your library",
  });
  cleanup();
  linked = [
    { occurrence_id: "opaque:occurrence/play/1", recording: foundRecording(0) },
  ];
  trackBodies.length = 0;
  const again = await openPage();
  expect(trackBodies).toHaveLength(1);
  expect(
    await within(rowOf(again, title)).findByRole("img", {
      name: "In your library",
    }),
  ).toBeTruthy();
});

test("a cold direct album link loads its editions and tracks even when the artist profile request fails", async () => {
  profileFails = true;
  const fetch = serve();
  render(
    createElement(CatalogAlbumPage, {
      ...artistIdentityBrowsing().detail,
      release_group_id: editionGroup.id,
    }),
  );
  const list = await screen.findByRole("list", { name: "Tracks" });
  await within(list).findByText(catalogAlbumTitles[0]);
  const editions = fetch.mock.calls.filter(
    ([path]) =>
      path === `${base}/${artistAssociation.id}/release-group-editions`,
  );
  expect(editions.length).toBeGreaterThan(0);
  expect(JSON.parse(editions[0][1]!.body as string)).toMatchObject({
    release_group_id: editionGroup.id,
  });
  expect(
    screen.getByRole("heading", { level: 1 }).textContent?.trim(),
  ).toBeTruthy();
});
