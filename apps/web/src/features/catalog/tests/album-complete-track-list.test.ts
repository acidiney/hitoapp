import type { Schema } from "../../../shared/schema";
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
import { pickRelease, confirmEdition } from "../fixtures/albumEditionSteps";

import {
  albumDetail,
  albumId,
  attachedTrackList,
  artistAssociation,
  artistReviewAccount,
  editionGroup,
  firstDiscographyPage,
  firstEditionPage,
  firstLibraryRecording,
  installation,
  mergedTrackList,
  personalTitle,
  repeatedCatalogTrackList,
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
    queueSnapshot: () => ({ currentIndex: null, total: 0 }),
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
const audio = `/api/v1/albums/${albumId}/edition-track-audio`;

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

function page(detail: Schema<"AlbumPageProps">["detail"] = albumDetail) {
  return createElement(AlbumPage, {
    errors: {},
    native_playback: false,
    lock: { pin_enabled: false, idle_minutes: 0, locked: false },
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
  await pickRelease();
}

const chooseEdition = async () => {
  await openEditions();
  return confirmEdition();
};

function rowWith(region: HTMLElement, text: string) {
  return within(region)
    .getAllByRole("listitem")
    .find(
      (item) =>
        item.textContent?.includes(text) ||
        item.querySelector(`[aria-label="Play ${text}"]`),
    )!;
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
      .map(
        (row) =>
          (row.textContent ?? "") +
          [...row.querySelectorAll("[aria-label]")]
            .map((node) => node.getAttribute("aria-label"))
            .join(" "),
      );
  const first = rows(discs[0]);
  expect(first[0]).toContain("Hidden intro");
  expect(first[0]).toContain("Pregap");
  expect(first[1]).toMatch(/Ready|In your library/);
  expect(first[1]).toContain(personalTitle);
  expect(first[1]).toContain("Catalog opening");
  expect(first[2]).toContain("Not in your library");
  expect(first[2]).toContain("Catalog only song");
  const second = rows(discs[1]);
  expect(second[0]).toMatch(/Ready|In your library/);
  expect(second[0]).toContain(personalTitle);
  expect(second[1]).toContain("Unidentified bonus");
  expect(second[1]).toContain("No provider identity for this track");
  expect(second).toHaveLength(3);
  expect(second[2]).toContain("Library only");
  expect(second[2]).toMatch(/Ready|In your library/);
  expect(within(region).queryByText(/Find audio and play/)).toBeNull();
  await waitFor(() =>
    expect(screen.queryByRole("table", { name: "Tracks" })).toBeNull(),
  );
  expect(
    screen.queryByRole("heading", { name: "Complete track list" }),
  ).toBeNull();
  expect(
    within(region).queryByText(/saved to your library only when/),
  ).toBeNull();
  expect(fetch.mock.calls.some(([path]) => path === adoption)).toBe(false);
  expect(player.playQueue).not.toHaveBeenCalled();
});

test("play on a catalog-only track plays the album from that row through the edition playback, not a provider save", async () => {
  const fetch = serve({
    [audio]: () =>
      Response.json({ status: "saved", recording: savedRecording }),
  });
  player.playSelection.mockClear();
  player.playQueue.mockClear();
  render(page());
  await chooseEdition();
  await screen.findByRole("button", { name: "Try saving again" });
  const region = screen.getByRole("region", {
    name: "Album edition track list",
  });
  const row = rowWith(region, "Catalog only song");
  await act(async () => {
    fireEvent.click(
      within(row).getByRole("button", { name: "Play Catalog only song" }),
    );
  });
  await waitFor(() => expect(player.playSelection).toHaveBeenCalledTimes(1));
  const selection = player.playSelection.mock.calls[0][0] as {
    queue: Schema<"Recording">[];
  };
  expect(selection.queue).toEqual([savedRecording]);
  expect(fetch.mock.calls.filter(([path]) => path === adoption)).toHaveLength(
    0,
  );
  expect(player.playQueue).not.toHaveBeenCalled();
});

test("add on a catalog-only track saves it through its provider without playing", async () => {
  const fetch = serve();
  player.playSelection.mockClear();
  render(page());
  const region = await chooseEdition();
  const row = rowWith(region, "Catalog only song");
  await act(async () => {
    fireEvent.click(
      within(row).getByRole("button", {
        name: "Add Catalog only song to your library",
      }),
    );
  });
  await waitFor(() => expect(row.textContent).toMatch(/Ready|In your library/));
  const saves = fetch.mock.calls.filter(([path]) => path === adoption);
  expect(saves).toHaveLength(1);
  expect(JSON.parse(saves[0][1]!.body as string)).toEqual({
    installation_revision: installation.revision,
    provider_recording_id: "provider:two",
  });
  expect(player.playSelection).not.toHaveBeenCalled();
  expect(
    within(row).queryByRole("button", {
      name: "Add Catalog only song to your library",
    }),
  ).toBeNull();
});

test("a library track starts the album queue from its row without calling a provider first", async () => {
  const fetch = serve();
  player.playSelection.mockClear();
  player.playQueue.mockClear();
  render(page());
  await chooseEdition();
  await screen.findByRole("button", { name: "Try saving again" });
  const region = screen.getByRole("region", {
    name: "Album edition track list",
  });
  const calls = fetch.mock.calls.length;
  const row = rowWith(region, personalTitle);
  await act(async () => {
    fireEvent.click(within(row).getByRole("button", { name: /^Play / }));
  });
  await waitFor(() => expect(player.playSelection).toHaveBeenCalledTimes(1));
  const selection = player.playSelection.mock.calls[0][0] as {
    queue: Schema<"Recording">[];
  };
  expect(selection.queue.map((track) => track.id)).toEqual([
    firstLibraryRecording.id,
  ]);
  expect(player.playQueue).not.toHaveBeenCalled();
  expect(fetch.mock.calls.length).toBe(calls);
});

test("a provider outage is honest and leaves the album's library tracks playable", async () => {
  serve({ [comparison]: () => Response.json({ status: "unavailable" }) });
  render(page());
  await openEditions();
  fireEvent.click(screen.getByRole("button", { name: "Use this" }));
  const alert = await screen.findByText(/track list is unavailable/);
  expect(alert.getAttribute("role")).toBe("alert");
  expect(
    screen.getByRole("button", { name: `Play ${personalTitle}` }),
  ).toBeTruthy();
});

test("catalog tracks that no connected provider can save still play with the album and add by finding audio", async () => {
  const fetch = serve({
    [comparison]: () => Response.json(unsavableTrackList),
  });
  render(page());
  const region = await chooseEdition();
  const row = rowWith(region, "Catalog only song");
  expect(
    within(row).getByRole("button", { name: "Play Catalog only song" }),
  ).toBeTruthy();
  expect(
    within(row).getByRole("button", {
      name: "Add Catalog only song to your library",
    }),
  ).toBeTruthy();
  expect(fetch.mock.calls.some(([path]) => path === adoption)).toBe(false);
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
  await openEditions();
  fireEvent.click(screen.getByRole("button", { name: "Use this" }));
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
      name: "Add Catalog only song to your library",
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

test("a provider id attached to another recording is reported honestly and offers no Play", async () => {
  const fetch = serve({ [comparison]: () => Response.json(attachedTrackList) });
  render(page());
  const region = await chooseEdition();
  const row = rowWith(region, "Catalog only song");
  expect(within(row).queryByRole("button")).toBeNull();
  expect(row.textContent).toContain("Not in your library");
  expect(row.textContent).toContain(
    "Already attached as audio to another recording",
  );
  expect(fetch.mock.calls.some(([path]) => path === adoption)).toBe(false);
});

test("saving state and failures belong to the clicked occurrence, not to every row with the same provider id", async () => {
  let fail!: (response: Response) => void;
  const held = new Promise<Response>((resolve) => {
    fail = resolve;
  });
  serve({
    [comparison]: () => Response.json(repeatedCatalogTrackList),
    [adoption]: () => held,
  });
  render(page());
  const region = await chooseEdition();
  const rows = within(region)
    .getAllByRole("listitem")
    .filter((item) => item.textContent?.includes("Not in your library"));
  const [first, second] = rows.filter(
    (item) =>
      item.textContent?.includes("Catalog only song") ||
      item.textContent?.includes("Catalog song again"),
  );
  fireEvent.click(within(first).getByRole("button", { name: /^Add / }));
  await waitFor(() => expect(first.textContent).toContain("Saving…"));
  expect(second.textContent).not.toContain("Saving…");
  await act(async () =>
    fail(Response.json({ error: "Provider refused" }, { status: 500 })),
  );
  await waitFor(() => expect(within(first).getByRole("alert")).toBeTruthy());
  expect(within(second).queryByRole("alert")).toBeNull();
});

test("opening the complete track list never renders the artist's whole discography", async () => {
  serve();
  render(page());
  fireEvent.click(
    screen.getByRole("button", { name: "Show complete track list" }),
  );
  await screen.findByRole("heading", {
    name: "Which release is Catalog album?",
  });
  expect(screen.queryByRole("region", { name: "Discography" })).toBeNull();
  expect(
    screen.queryByRole("list", { name: "Discography releases" }),
  ).toBeNull();
  expect(screen.queryByRole("heading", { name: "Discography" })).toBeNull();
  expect(screen.queryByText(editionGroup.title)).toBeNull();
});

test("library tracks outside the edition end the same track list as owned rows, with no separate block", async () => {
  serve();
  render(page());
  const region = await chooseEdition();
  expect(
    within(region).queryByRole("heading", {
      name: "In your library, not in this edition",
    }),
  ).toBeNull();
  expect(
    within(region).queryByRole("list", { name: "In this album only" }),
  ).toBeNull();
  expect(
    within(region).queryByRole("region", { name: "Not in this edition" }),
  ).toBeNull();
  const lastDisc = within(region).getByRole("list", {
    name: "Disc 2 tracks",
  });
  const rows = within(lastDisc).getAllByRole("listitem");
  const last = rows.at(-1)!;
  expect(last.textContent).toContain("Library only");
  expect(last.textContent).toMatch(/Ready|In your library/);
  fireEvent.click(
    within(last).getByRole("button", { name: "Play Library only" }),
  );
  expect(player.playQueue.mock.calls.at(-1)![0][0].id).toBe("recording-extra");
  fireEvent.click(screen.getByRole("button", { name: /^In my library \d+$/ }));
  expect(within(region).getByText("Library only")).toBeTruthy();
});

test("the edition lives in one chip row with filters and a menu, and the header reports how much is owned", async () => {
  serve();
  render(page());
  const region = await chooseEdition();
  expect(screen.getByRole("button", { name: /^All \d+$/ })).toBeTruthy();
  const mine = screen.getByRole("button", { name: /^In my library \d+$/ });
  fireEvent.click(mine);
  expect(mine.getAttribute("aria-pressed")).toBe("true");
  expect(within(region).queryByText("Catalog only song")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: /^Edition / }));
  expect(
    screen.getAllByRole("menuitem").map((item) => item.textContent),
  ).toEqual(["Change edition"]);
  expect(screen.queryByText(/^Lined up with/)).toBeNull();
  expect(
    screen.getByRole("img", { name: /tracks in your library$/ }),
  ).toBeTruthy();
});
