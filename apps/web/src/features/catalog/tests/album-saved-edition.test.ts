import { createElement, type AnchorHTMLAttributes } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import AlbumPage from "../pages/AlbumPage";
import type { AlbumPageProps } from "../../../shared/types";
import {
  albumDetail,
  albumId,
  artistAssociation,
  artistReviewAccount,
  choiceReceipt,
  editionGroup,
  firstDiscographyPage,
  firstEditionPage,
  firstLibraryRecording,
  mergedTrackList,
  readyChoice,
  reviewedArtistProfile,
  savedEdition,
  staleChoice,
  standardEdition,
} from "../fixtures/albumEditions";

vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({
    playQueue: vi.fn(),
    playSelection: vi.fn(),
    prepareSelection: vi.fn(),
    subscribe: () => () => {},
    snapshot: () => ({ current: null }),
  }),
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
const savedRead = `/api/v1/albums/${albumId}/edition/track-list`;
const choose = `/api/v1/albums/${albumId}/edition`;
const clear = `/api/v1/albums/${albumId}/edition/clear`;

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
    [savedRead]: () => Response.json(mergedTrackList),
    [choose]: () => Response.json(choiceReceipt(4)),
    [clear]: () =>
      Response.json({ ...choiceReceipt(4), edition: null, revision: 4 }),
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

function page(
  saved: AlbumPageProps["detail"]["saved_edition"],
  anchor: AlbumPageProps["detail"]["catalog_anchor"] = albumDetail.catalog_anchor,
) {
  return createElement(AlbumPage, {
    errors: {},
    native_playback: false,
    lock: { pin_enabled: false, idle_minutes: 0, locked: false },
    user: { id: artistReviewAccount, username: "Listener", admin: false },
    libraries: [],
    playlists: [],
    detail: { ...albumDetail, saved_edition: saved, catalog_anchor: anchor },
  });
}

const calls = (fetch: ReturnType<typeof serve>, path: string) =>
  fetch.mock.calls.filter(([called]) => called === path);

async function chooseFirstEdition() {
  const groups = await screen.findByRole("list", {
    name: "Discography releases",
  });
  const group = within(groups).getAllByRole("listitem")[0];
  fireEvent.click(
    within(group).getByRole("button", { name: "Browse editions" }),
  );
  await within(group).findByRole("list", { name: "Available editions" });
  fireEvent.click(
    within(group).getByRole("button", { name: "Choose edition" }),
  );
  return screen.findByRole("region", { name: "Album edition track list" });
}

afterEach(() => vi.unstubAllGlobals());

test("a remembered edition opens with one action and no discography or edition browsing", async () => {
  const fetch = serve();
  render(page(readyChoice));
  expect(fetch).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", { name: "Show complete track list" }),
  );
  const region = await screen.findByRole("region", {
    name: "Album edition track list",
  });
  expect(within(region).getByText("Catalog only song")).toBeTruthy();
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([savedRead]);
  expect(fetch.mock.calls[0][1]!.method).toBe("POST");
  expect(screen.getByRole("button", { name: "Change edition" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Forget edition" })).toBeTruthy();
});

test("choosing an edition remembers it with the displayed revision and the next command uses the new one", async () => {
  const fetch = serve();
  render(page({ revision: 3, state: { status: "unset" } }));
  fireEvent.click(
    screen.getByRole("button", { name: "Show complete track list" }),
  );
  await chooseFirstEdition();
  await waitFor(() => expect(calls(fetch, choose)).toHaveLength(1));
  const body = JSON.parse(calls(fetch, choose)[0][1]!.body as string);
  expect(body).toEqual({
    request_id: expect.stringMatching(/^[0-9a-f-]{36}$/),
    expected_revision: 3,
    ...savedEdition,
  });
  expect(calls(fetch, choose)[0][1]!.method).toBe("PUT");
  expect(await screen.findByText("This edition is remembered.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Forget edition" })).toBeTruthy();

  fireEvent.click(screen.getByRole("button", { name: "Forget edition" }));
  await waitFor(() => expect(calls(fetch, clear)).toHaveLength(1));
  expect(
    JSON.parse(calls(fetch, clear)[0][1]!.body as string).expected_revision,
  ).toBe(4);
});

test("forgetting the edition clears it and returns to an explicit choice", async () => {
  const fetch = serve();
  render(page(readyChoice));
  fireEvent.click(
    screen.getByRole("button", { name: "Show complete track list" }),
  );
  await screen.findByRole("region", { name: "Album edition track list" });
  fireEvent.click(screen.getByRole("button", { name: "Forget edition" }));
  await waitFor(() => expect(calls(fetch, clear)).toHaveLength(1));
  expect(JSON.parse(calls(fetch, clear)[0][1]!.body as string)).toEqual({
    request_id: expect.stringMatching(/^[0-9a-f-]{36}$/),
    expected_revision: 3,
  });
  await screen.findByRole("button", { name: "Show complete track list" });
  expect(
    screen.queryByRole("region", { name: "Album edition track list" }),
  ).toBeNull();
  expect(screen.queryByRole("button", { name: "Forget edition" })).toBeNull();
  fireEvent.click(
    screen.getByRole("button", { name: "Show complete track list" }),
  );
  await screen.findByRole("list", { name: "Discography releases" });
  expect(calls(fetch, savedRead)).toHaveLength(1);
});

test("changing the edition opens the browser while the remembered one stays until another is chosen", async () => {
  const fetch = serve();
  render(
    page({
      revision: 3,
      state: {
        status: "ready",
        edition: { ...savedEdition, edition_id: "opaque:edition/other" },
      },
    }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Show complete track list" }),
  );
  await screen.findByRole("region", { name: "Album edition track list" });
  fireEvent.click(screen.getByRole("button", { name: "Change edition" }));
  await screen.findByRole("list", { name: "Discography releases" });
  expect(calls(fetch, choose)).toHaveLength(0);
  expect(calls(fetch, clear)).toHaveLength(0);
  await chooseFirstEdition();
  await waitFor(() => expect(calls(fetch, choose)).toHaveLength(1));
  expect(
    JSON.parse(calls(fetch, choose)[0][1]!.body as string).expected_revision,
  ).toBe(3);
});

test("a remembered edition that can no longer be read asks for a new choice without hunting for it", async () => {
  const fetch = serve();
  render(page(staleChoice));
  expect(screen.getByText(/saved edition can no longer be used/)).toBeTruthy();
  fireEvent.click(
    screen.getByRole("button", { name: "Show complete track list" }),
  );
  await screen.findByRole("list", { name: "Discography releases" });
  expect(calls(fetch, savedRead)).toHaveLength(0);
  expect(editionGroup.id).toBeTruthy();
  expect(standardEdition.id).toBeTruthy();
});

test("a link that the server reports as stale while reading falls back to choosing an edition", async () => {
  const fetch = serve({
    [savedRead]: () => Response.json({ status: "needs_choice" }),
  });
  render(page(readyChoice));
  fireEvent.click(
    screen.getByRole("button", { name: "Show complete track list" }),
  );
  await screen.findByRole("list", { name: "Discography releases" });
  expect(calls(fetch, savedRead)).toHaveLength(1);
  expect(screen.getByText(/saved edition can no longer be used/)).toBeTruthy();
});

test("a failed save is reported while the compared track list stays visible", async () => {
  serve({
    [choose]: () =>
      Response.json({ error: "Album save refused" }, { status: 500 }),
  });
  render(page({ revision: 0, state: { status: "unset" } }));
  fireEvent.click(
    screen.getByRole("button", { name: "Show complete track list" }),
  );
  const region = await chooseFirstEdition();
  const alert = await screen.findByText(/Album save refused/);
  expect(alert.getAttribute("role")).toBe("alert");
  expect(within(region).getByText("Catalog only song")).toBeTruthy();
});

test("leaving the album cancels a held remembered read", async () => {
  let release!: (response: Response) => void;
  const held = new Promise<Response>((resolve) => {
    release = resolve;
  });
  const signals: AbortSignal[] = [];
  serve({
    [savedRead]: (init) => {
      signals.push(init!.signal!);
      return held;
    },
  });
  const view = render(page(readyChoice));
  fireEvent.click(
    screen.getByRole("button", { name: "Show complete track list" }),
  );
  await waitFor(() => expect(signals).toHaveLength(1));
  const errors = vi.spyOn(console, "error");
  view.unmount();
  expect(signals[0].aborted).toBe(true);
  await act(async () => release(Response.json(mergedTrackList)));
  expect(errors).not.toHaveBeenCalled();
});

test("a later page of the album has no remembered edition to show", () => {
  serve();
  render(page(null, null));
  expect(
    screen.queryByRole("button", { name: "Show complete track list" }),
  ).toBeNull();
});

test("a stale remembered edition can be forgotten even when the album has no reviewed artist", async () => {
  const fetch = serve();
  render(page(staleChoice, null));
  expect(screen.getByText(/needs a reviewed artist/)).toBeTruthy();
  expect(
    screen.queryByRole("button", { name: "Show complete track list" }),
  ).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Forget edition" }));
  await waitFor(() => expect(calls(fetch, clear)).toHaveLength(1));
  expect(JSON.parse(calls(fetch, clear)[0][1]!.body as string)).toMatchObject({
    expected_revision: 3,
  });
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Forget edition" })).toBeNull(),
  );
});

test("a link that turns stale while reading with no reviewed artist can still be forgotten", async () => {
  serve({ [savedRead]: () => Response.json({ status: "needs_choice" }) });
  render(page(readyChoice, null));
  fireEvent.click(
    screen.getByRole("button", { name: "Show complete track list" }),
  );
  await screen.findByText(/needs a reviewed artist/);
  expect(screen.getByRole("button", { name: "Forget edition" })).toBeTruthy();
});
