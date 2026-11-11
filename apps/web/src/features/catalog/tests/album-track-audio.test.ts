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
import { openFromAlbum } from "../fixtures/albumEditionSteps";
import AlbumEditionTrackList from "../../artists/components/AlbumEditionTrackList";
import {
  albumDetail,
  albumId,
  artistAssociation,
  artistReviewAccount,
  editionGroup,
  firstDiscographyPage,
  firstEditionPage,
  firstLibraryRecording,
  reviewedArtistProfile,
  savedRecording,
  standardEdition,
  personalTitle,
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
const audio = `/api/v1/albums/${albumId}/edition-track-audio`;
const found = { status: "saved", recording: savedRecording };
const anyAdoption = /\/api\/v1\/plugins\/.*\/recordings$/;

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
    [comparison]: () => Response.json(unsavableTrackList),
    [audio]: () => Response.json(found),
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

function page() {
  return createElement(AlbumPage, {
    errors: {},
    native_playback: false,
    lock: { pin_enabled: false, idle_minutes: 0, locked: false },
    user: { id: artistReviewAccount, username: "Listener", admin: false },
    libraries: [],
    playlists: [],
    detail: albumDetail,
  });
}

const chooseEdition = openFromAlbum;

function rowWith(region: HTMLElement, text: string) {
  return within(region)
    .getAllByRole("listitem")
    .find((item) => item.textContent?.includes(text))!;
}

const findButton = (row: HTMLElement) =>
  within(row).getByRole("button", { name: "Find audio for Catalog only song" });

test("finding audio sends only identities, the server saves the match and the saved recording plays", async () => {
  const fetch = serve();
  render(page());
  const region = await chooseEdition();
  const row = rowWith(region, "Catalog only song");
  await act(async () => {
    fireEvent.click(findButton(row));
  });
  await waitFor(() => expect(player.playSelection).toHaveBeenCalledTimes(1));
  const searches = fetch.mock.calls.filter(([path]) => path === audio);
  expect(searches).toHaveLength(1);
  expect(JSON.parse(searches[0][1]!.body as string)).toEqual({
    recording_id: firstLibraryRecording.id,
    association_id: artistAssociation.id,
    release_group_id: editionGroup.id,
    edition_id: standardEdition.id,
    occurrence_id: "occ-1-2",
  });
  expect(
    fetch.mock.calls.some(([path]) => anyAdoption.test(String(path))),
  ).toBe(false);
  expect(player.playSelection).toHaveBeenCalledWith(savedRecording);
  await waitFor(() => expect(row.textContent).toContain("In your library"));
});

test("a search refused because automatic service search is off says how to turn it on", async () => {
  serve({ [audio]: () => Response.json({ status: "search_off" }) });
  render(page());
  const region = await chooseEdition();
  const row = rowWith(region, "Catalog only song");
  await act(async () => {
    fireEvent.click(findButton(row));
  });
  await waitFor(() =>
    expect(within(row).getByRole("alert").textContent).toContain(
      "Search connected services automatically",
    ),
  );
  expect(player.playSelection).not.toHaveBeenCalled();
});

test("an uncertain match saves nothing, says so and can be retried", async () => {
  let answer: unknown = { status: "no_match" };
  const fetch = serve({ [audio]: () => Response.json(answer) });
  render(page());
  const region = await chooseEdition();
  const row = rowWith(region, "Catalog only song");
  await act(async () => {
    fireEvent.click(findButton(row));
  });
  const alert = await within(row).findByRole("alert");
  expect(alert.textContent).toContain("confident match");
  expect(alert.textContent).toContain("Nothing was saved");
  expect(
    fetch.mock.calls.some(([path]) => anyAdoption.test(String(path))),
  ).toBe(false);
  expect(player.playSelection).not.toHaveBeenCalled();
  answer = found;
  await act(async () => {
    fireEvent.click(findButton(row));
  });
  await waitFor(() => expect(player.playSelection).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(within(row).queryByRole("alert")).toBeNull());
});

test("an audio provider outage is honest and retryable while the album stays playable", async () => {
  let answer: unknown = { status: "unavailable" };
  serve({ [audio]: () => Response.json(answer) });
  render(page());
  const region = await chooseEdition();
  const row = rowWith(region, "Catalog only song");
  await act(async () => {
    fireEvent.click(findButton(row));
  });
  const alert = await within(row).findByRole("alert");
  expect(alert.textContent).toContain("unavailable");
  expect(findButton(row).hasAttribute("disabled")).toBe(false);
  answer = found;
  await act(async () => {
    fireEvent.click(findButton(row));
  });
  await waitFor(() => expect(player.playSelection).toHaveBeenCalledTimes(1));
});

test("without a connected audio provider the track explains what to connect", async () => {
  serve({ [audio]: () => Response.json({ status: "no_providers" }) });
  render(page());
  const region = await chooseEdition();
  const row = rowWith(region, "Catalog only song");
  await act(async () => {
    fireEvent.click(findButton(row));
  });
  expect((await within(row).findByRole("alert")).textContent).toContain(
    "Connect an audio provider",
  );
  expect(player.playSelection).not.toHaveBeenCalled();
});

test("an unrecognised answer is reported as invalid and saves nothing", async () => {
  const fetch = serve({ [audio]: () => Response.json({ status: "surprise" }) });
  render(page());
  const region = await chooseEdition();
  const row = rowWith(region, "Catalog only song");
  await act(async () => {
    fireEvent.click(findButton(row));
  });
  expect((await within(row).findByRole("alert")).textContent).toContain(
    "invalid",
  );
  expect(
    fetch.mock.calls.some(([path]) => anyAdoption.test(String(path))),
  ).toBe(false);
});

test("leaving the album cancels a held audio search and starts no playback", async () => {
  let release!: (response: Response) => void;
  const held = new Promise<Response>((resolve) => {
    release = resolve;
  });
  const signals: AbortSignal[] = [];
  const fetch = serve({
    [audio]: (init) => {
      signals.push(init!.signal!);
      return held;
    },
  });
  const view = render(page());
  const region = await chooseEdition();
  fireEvent.click(findButton(rowWith(region, "Catalog only song")));
  await waitFor(() => expect(signals).toHaveLength(1));
  view.unmount();
  expect(signals[0].aborted).toBe(true);
  await act(async () => release(Response.json(found)));
  expect(
    fetch.mock.calls.some(([path]) => anyAdoption.test(String(path))),
  ).toBe(false);
  expect(player.playSelection).not.toHaveBeenCalled();
});

test("busy, insufficient evidence and a changed provider answer are each reported honestly", async () => {
  let answer: unknown = { status: "busy" };
  serve({ [audio]: () => Response.json(answer) });
  render(page());
  const region = await chooseEdition();
  const row = rowWith(region, "Catalog only song");
  for (const [status, text] of [
    ["busy", "Another audio search is running"],
    ["insufficient_evidence", "single artist and a length"],
    ["changed", "changed while saving"],
  ]) {
    answer = { status };
    await act(async () => {
      fireEvent.click(findButton(row));
    });
    expect((await within(row).findByRole("alert")).textContent).toContain(text);
  }
  expect(player.playSelection).not.toHaveBeenCalled();
});

test("a result found for one edition is not shown for another edition with the same occurrence ids", async () => {
  const fetch = serve();
  const base = unsavableTrackList;
  const edition = (id: string) =>
    ({ ...base, edition: { ...base.edition, id } }) as typeof base;
  const view = render(
    createElement(AlbumEditionTrackList, {
      account: artistReviewAccount,
      source: { release: albumId, recording: firstLibraryRecording.id },
      comparison: edition("edition-a"),
      pending: false,
      error: "",
      retry: async () => {},
    }),
  );
  const first = screen.getByRole("region", {
    name: "Album edition track list",
  });
  await act(async () => {
    fireEvent.click(findButton(rowWith(first, "Catalog only song")));
  });
  await waitFor(() =>
    expect(rowWith(first, "Catalog only song").textContent).toContain(
      "In your library",
    ),
  );
  view.rerender(
    createElement(AlbumEditionTrackList, {
      account: artistReviewAccount,
      source: { release: albumId, recording: firstLibraryRecording.id },
      comparison: edition("edition-b"),
      pending: false,
      error: "",
      retry: async () => {},
    }),
  );
  const second = screen.getByRole("region", {
    name: "Album edition track list",
  });
  const row = rowWith(second, "Catalog only song");
  expect(row.textContent).not.toContain("In your library");
  expect(findButton(row)).toBeTruthy();
  const searches = fetch.mock.calls.filter(([path]) => path === audio);
  expect(searches).toHaveLength(1);
  await act(async () => {
    fireEvent.click(findButton(row));
  });
  await waitFor(() =>
    expect(fetch.mock.calls.filter(([path]) => path === audio)).toHaveLength(2),
  );
});

test("switching editions aborts an in-flight search without saving or playing", async () => {
  let release!: (response: Response) => void;
  const held = new Promise<Response>((resolve) => {
    release = resolve;
  });
  const signals: AbortSignal[] = [];
  serve({
    [audio]: (init) => {
      signals.push(init!.signal!);
      return held;
    },
  });
  const props = (id: string) => ({
    account: artistReviewAccount,
    source: { release: albumId, recording: firstLibraryRecording.id },
    comparison: {
      ...unsavableTrackList,
      edition: { ...unsavableTrackList.edition, id },
    } as typeof unsavableTrackList,
    pending: false,
    error: "",
    retry: async () => {},
  });
  const view = render(createElement(AlbumEditionTrackList, props("edition-a")));
  const region = screen.getByRole("region", {
    name: "Album edition track list",
  });
  fireEvent.click(findButton(rowWith(region, "Catalog only song")));
  await waitFor(() => expect(signals).toHaveLength(1));
  view.rerender(createElement(AlbumEditionTrackList, props("edition-b")));
  expect(signals[0].aborted).toBe(true);
  await act(async () => release(Response.json(found)));
  expect(player.playSelection).not.toHaveBeenCalled();
});

test("a track in the library never searches providers", async () => {
  const fetch = serve({
    [comparison]: () =>
      Response.json({
        ...unsavableTrackList,
        matches: [
          { occurrence_id: "occ-1-2", recording: firstLibraryRecording },
        ],
      }),
  });
  render(page());
  const region = await chooseEdition();
  const row = rowWith(region, personalTitle);
  expect(within(row).queryByRole("button", { name: /^Find audio/ })).toBeNull();
  expect(fetch.mock.calls.some(([path]) => path === audio)).toBe(false);
});
