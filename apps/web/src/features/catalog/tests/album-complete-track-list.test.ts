import { createElement, type AnchorHTMLAttributes } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import AlbumPage from "../pages/AlbumPage";
import type { AlbumPageProps } from "../../../shared/types";
import {
  albumDetail,
  albumId,
  artistAssociation,
  artistReviewAccount,
  editionGroup,
  firstDiscographyPage,
  firstEditionPage,
  firstLibraryRecording,
  installation,
  mergedTrackList,
  personalTitle,
  reviewedArtistProfile,
  savedRecording,
  standardEdition,
  unsavableTrackList,
} from "../fixtures/albumEditions";

const player = vi.hoisted(() => {
  const playSelection = vi.fn(async (_target: unknown) => true);
  return {
    playQueue: vi.fn(),
    playSelection,
    prepareSelection: vi.fn(() => playSelection),
    subscribe: () => () => {},
    snapshot: () => ({ current: null }),
  };
});
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn() },
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

const associations = `/api/v1/recordings/${firstLibraryRecording.id}/artist-associations`;
const comparison = `/api/v1/albums/${albumId}/edition-track-list`;
const adoption = `/api/v1/plugins/${installation.id}/recordings`;

type Route = (init?: RequestInit) => Response | Promise<Response>;

function serve(overrides: Record<string, Route> = {}) {
  const routes: Record<string, Route> = {
    [associations]: () => Response.json([artistAssociation]),
    [`${associations}/${artistAssociation.id}/profile`]: () =>
      Response.json(reviewedArtistProfile),
    [`${associations}/${artistAssociation.id}/discography`]: () =>
      Response.json(firstDiscographyPage),
    [`${associations}/${artistAssociation.id}/release-group-editions`]: () =>
      Response.json({ ...firstEditionPage, continuation: null }),
    [comparison]: () => Response.json(mergedTrackList),
    [adoption]: () => Response.json(savedRecording),
    ...overrides,
  };
  const fetch = vi.fn(async (path: string, init?: RequestInit) => {
    const route = routes[path];
    if (!route) throw new Error(`Unexpected request ${path}`);
    return route(init);
  });
  vi.stubGlobal("fetch", fetch);
  return fetch;
}

function page(detail: AlbumPageProps["detail"] = albumDetail) {
  return createElement(AlbumPage, {
    errors: {},
    native_playback: false,
    user: { id: artistReviewAccount, username: "Listener", admin: false },
    libraries: [],
    playlists: [],
    detail,
  });
}

async function openEditions() {
  fireEvent.click(
    screen.getByRole("button", { name: "Show complete track list" }),
  );
  const groups = await screen.findByRole("list", {
    name: "Discography releases",
  });
  const group = within(groups).getAllByRole("listitem")[0];
  fireEvent.click(
    within(group).getByRole("button", { name: "Browse editions" }),
  );
  await within(group).findByRole("list", { name: "Available editions" });
  return group;
}

async function chooseEdition() {
  const group = await openEditions();
  fireEvent.click(
    within(group).getByRole("button", { name: "Choose edition" }),
  );
  return screen.findByRole("region", { name: "Album edition track list" });
}

function rowWith(region: HTMLElement, text: string) {
  return within(region)
    .getAllByRole("listitem")
    .find((item) => item.textContent?.includes(text))!;
}

test("the complete track list is explicit, ordered, and distinguishes library tracks from catalog-only tracks", async () => {
  const fetch = serve();
  render(page());
  expect(fetch).not.toHaveBeenCalled();

  const region = await chooseEdition();
  expect(JSON.parse(fetch.mock.calls.at(-1)![1]!.body as string)).toEqual({
    recording_id: firstLibraryRecording.id,
    association_id: artistAssociation.id,
    release_group_id: editionGroup.id,
    edition_id: standardEdition.id,
  });
  const discs = within(region).getAllByRole("region", { name: /^Disc \d$/ });
  expect(discs.map((disc) => disc.getAttribute("aria-label"))).toEqual([
    "Disc 1",
    "Disc 2",
  ]);
  const rows = (disc: HTMLElement) =>
    within(disc)
      .getAllByRole("listitem")
      .map((row) => row.textContent ?? "");
  const first = rows(discs[0]);
  expect(first[0]).toContain("Hidden intro");
  expect(first[0]).toContain("Pregap");
  expect(first[1]).toContain("In your library");
  expect(first[1]).toContain(personalTitle);
  expect(first[1]).toContain("Catalog opening");
  expect(first[2]).toContain("Not in your library");
  expect(first[2]).toContain("Catalog only song");
  const second = rows(discs[1]);
  expect(second[0]).toContain("In your library");
  expect(second[0]).toContain(personalTitle);
  expect(second[1]).toContain("Unidentified bonus");
  expect(second[1]).toContain("No provider identity for this track");
  expect(
    within(region).getByText("This is the release this album was saved from."),
  ).toBeTruthy();
  expect(
    within(
      within(region).getByRole("list", { name: "In this album only" }),
    ).getAllByRole("listitem"),
  ).toHaveLength(1);
  expect(fetch.mock.calls.some(([path]) => path === adoption)).toBe(false);
  expect(player.playQueue).not.toHaveBeenCalled();
});

test("play on a catalog-only track saves it through its provider once and then plays the saved recording", async () => {
  const fetch = serve();
  render(page());
  const region = await chooseEdition();
  const row = rowWith(region, "Catalog only song");
  await act(async () => {
    fireEvent.click(
      within(row).getByRole("button", { name: "Play Catalog only song" }),
    );
  });
  await waitFor(() => expect(player.playSelection).toHaveBeenCalledTimes(1));
  const saves = fetch.mock.calls.filter(([path]) => path === adoption);
  expect(saves).toHaveLength(1);
  expect(JSON.parse(saves[0][1]!.body as string)).toEqual({
    installation_revision: installation.revision,
    provider_recording_id: "provider:two",
  });
  expect(player.playSelection).toHaveBeenCalledWith(savedRecording);
  await waitFor(() => expect(row.textContent).toContain("In your library"));
});

test("a library track plays directly and never calls a provider", async () => {
  const fetch = serve();
  render(page());
  const region = await chooseEdition();
  const calls = fetch.mock.calls.length;
  const row = rowWith(region, personalTitle);
  fireEvent.click(within(row).getByRole("button", { name: /^Play / }));
  expect(player.playQueue).toHaveBeenCalledTimes(1);
  expect(player.playQueue.mock.calls[0][0][0].id).toBe(
    firstLibraryRecording.id,
  );
  expect(fetch.mock.calls.length).toBe(calls);
});

test("a provider outage is honest and leaves the album's library tracks playable", async () => {
  serve({ [comparison]: () => Response.json({ status: "unavailable" }) });
  render(page());
  const group = await openEditions();
  fireEvent.click(
    within(group).getByRole("button", { name: "Choose edition" }),
  );
  const alert = await screen.findByText(/track list is unavailable/);
  expect(alert.getAttribute("role")).toBe("alert");
  expect(
    screen.getByRole("button", { name: `Play ${personalTitle}` }),
  ).toBeTruthy();
});

test("catalog tracks that no connected provider can save have no Play action", async () => {
  serve({ [comparison]: () => Response.json(unsavableTrackList) });
  render(page());
  const region = await chooseEdition();
  const row = rowWith(region, "Catalog only song");
  expect(within(row).queryByRole("button")).toBeNull();
  expect(row.textContent).toContain(
    "No connected provider can save this track",
  );
});

test("leaving the album cancels a held comparison without updating or playing", async () => {
  let release!: (response: Response) => void;
  const held = new Promise<Response>((resolve) => {
    release = resolve;
  });
  const signals: AbortSignal[] = [];
  serve({
    [comparison]: (init) => {
      signals.push(init!.signal!);
      return held;
    },
  });
  const view = render(page());
  const group = await openEditions();
  fireEvent.click(
    within(group).getByRole("button", { name: "Choose edition" }),
  );
  await waitFor(() => expect(signals).toHaveLength(1));
  const errors = vi.spyOn(console, "error");
  view.unmount();
  expect(signals[0].aborted).toBe(true);
  await act(async () => release(Response.json(mergedTrackList)));
  expect(errors).not.toHaveBeenCalled();
  expect(player.playSelection).not.toHaveBeenCalled();
});

test("leaving the album cancels a held save and starts no playback", async () => {
  let release!: (response: Response) => void;
  const held = new Promise<Response>((resolve) => {
    release = resolve;
  });
  const signals: AbortSignal[] = [];
  serve({
    [adoption]: (init) => {
      signals.push(init!.signal!);
      return held;
    },
  });
  const view = render(page());
  const region = await chooseEdition();
  fireEvent.click(
    within(rowWith(region, "Catalog only song")).getByRole("button", {
      name: "Play Catalog only song",
    }),
  );
  await waitFor(() => expect(signals).toHaveLength(1));
  view.unmount();
  expect(signals[0].aborted).toBe(true);
  await act(async () => release(Response.json(savedRecording)));
  expect(player.playSelection).not.toHaveBeenCalled();
});

test("an album without a reviewed artist offers no complete track list", () => {
  serve();
  render(page({ ...albumDetail, catalog_anchor: null }));
  expect(
    screen.queryByRole("button", { name: "Show complete track list" }),
  ).toBeNull();
});
