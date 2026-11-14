import type { Schema } from "../../../shared/schema";
import { editionChip, editionMenuItem } from "../fixtures/editionMenu";
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
import {
  albumDetail,
  albumId,
  artistAssociation,
  artistReviewAccount,
  editionGroup,
  firstDiscographyPage,
  firstEditionPage,
  firstLibraryRecording,
  mergedTrackList,
  reviewedArtistProfile,
} from "../fixtures/albumEditions";
import { bonusEdition } from "../../artists/fixtures/artistEditions";

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
const editions = `${associations}/${artistAssociation.id}/release-group-editions`;
const comparison = `/api/v1/albums/${albumId}/edition-track-list`;
const choose = `/api/v1/albums/${albumId}/edition`;

type Route = (init?: RequestInit) => Response | Promise<Response>;

function serve(overrides: Record<string, Route> = {}) {
  const routes: Record<string, Route> = {
    [associations]: () => Response.json([artistAssociation]),
    [`${associations}/${artistAssociation.id}/profile`]: () =>
      Response.json(reviewedArtistProfile),
    [`${associations}/${artistAssociation.id}/discography`]: () =>
      Response.json({ ...firstDiscographyPage, continuation: null }),
    [editions]: () =>
      Response.json({ ...firstEditionPage, continuation: null }),
    [comparison]: () => Response.json(mergedTrackList),
    [choose]: () => Response.json({}),
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

const matched: Schema<"ReleaseDetail"> = {
  ...albumDetail,
  release: { ...albumDetail.release, title: editionGroup.title, year: 2024 },
};

function page(detail: Schema<"AlbumPageProps">["detail"]) {
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

const open = async () =>
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose an edition" }),
  );

afterEach(() => vi.unstubAllGlobals());

test("a release that is the only match with a single track list is used without asking", async () => {
  serve();
  render(page(matched));
  await open();
  await screen.findByRole("region", { name: "Album edition track list" });
  expect(await screen.findByRole("button", { name: /^Edition / })).toBeTruthy();
  await screen.findByRole("region", { name: "Album edition track list" });
  expect(
    screen.queryByRole("list", { name: "Releases to choose from" }),
  ).toBeNull();
  expect(screen.queryByRole("heading", { name: "Discography" })).toBeNull();
});

test("a matched release with several track lists asks which one, with the suggested release", async () => {
  const fetch = serve({
    [editions]: () =>
      Response.json({
        ...firstEditionPage,
        continuation: null,
        page: {
          ...firstEditionPage.page,
          items: [...firstEditionPage.page.items, bonusEdition],
        },
      }),
  });
  render(page(matched));
  await open();
  const dialog = await screen.findByRole("dialog", {
    name: `Is this ${editionGroup.title} from 2024?`,
  });
  const radios = await within(dialog).findAllByRole("radio");
  expect(radios).toHaveLength(2);
  expect((radios[0] as HTMLInputElement).checked).toBe(true);
  expect(fetch.mock.calls.map(([path]) => path)).not.toContain(comparison);
  fireEvent.click(radios[1]);
  fireEvent.click(within(dialog).getByRole("button", { name: "Use this" }));
  await screen.findByRole("region", { name: "Album edition track list" });
  expect(screen.queryByRole("dialog")).toBeNull();
  const call = fetch.mock.calls.find(([path]) => path === comparison)!;
  expect(JSON.parse(call[1]!.body as string).edition_id).toBe(bonusEdition.id);
});

test("it's a different release opens a styled release list to pick from", async () => {
  serve({
    [editions]: () =>
      Response.json({
        ...firstEditionPage,
        continuation: null,
        page: {
          ...firstEditionPage.page,
          items: [...firstEditionPage.page.items, bonusEdition],
        },
      }),
  });
  render(page(matched));
  await open();
  fireEvent.click(
    await screen.findByRole("button", { name: "It's a different release" }),
  );
  const list = await screen.findByRole("list", {
    name: "Releases to choose from",
  });
  expect(list.className).toBe("release-grid");
  expect(within(list).getAllByRole("listitem")).toHaveLength(1);
  fireEvent.click(
    within(list).getByRole("button", { name: `Choose ${editionGroup.title}` }),
  );
  await screen.findByRole("group", { name: /^Which track list\?/ });
});

test("just show my tracks closes the complete track list", async () => {
  serve();
  render(page(albumDetail));
  await open();
  fireEvent.click(
    await screen.findByRole("button", { name: "Just show my tracks" }),
  );
  expect(
    await screen.findByRole("button", { name: "Choose an edition" }),
  ).toBeTruthy();
});

test("editions that cannot be loaded leave only the album's tracks and offer Try again", async () => {
  let fail = true;
  serve({
    [editions]: () =>
      fail
        ? Response.json({ status: "unavailable" })
        : Response.json({ ...firstEditionPage, continuation: null }),
  });
  render(page(matched));
  await open();
  await screen.findByRole("heading", { name: "Showing only your tracks" });
  expect(screen.getByRole("alert").textContent).toContain(
    "Your 2 tracks are listed and play as usual",
  );
  fail = false;
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  await screen.findByRole("button", { name: /^Edition / });
});

test("a release with no editions says so and offers no retry", async () => {
  serve({
    [editions]: () =>
      Response.json({
        ...firstEditionPage,
        continuation: null,
        page: { ...firstEditionPage.page, items: [] },
      }),
  });
  render(page(matched));
  await open();
  await screen.findByRole("heading", {
    name: "No editions listed for this album",
  });
  expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "Just show my tracks" }),
    ).toBeTruthy(),
  );
});

test("while the release lookup runs the album keeps its own track list and shows only a small status line", async () => {
  serve({ [associations]: () => new Promise<Response>(() => {}) });
  render(page(matched));
  await open();
  const status = await screen.findByText("Looking for this album's release…");
  expect(status.getAttribute("role")).toBe("status");
  expect(
    screen.queryByRole("heading", { name: "Complete track list" }),
  ).toBeNull();
  expect(
    screen.queryByRole("heading", {
      name: "Finding the release of this album",
    }),
  ).toBeNull();
  expect(screen.getByRole("table", { name: "Tracks" })).toBeTruthy();
});

test("a stalled artist lookup resolves to Showing only your tracks instead of loading forever", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  try {
    serve({ [associations]: () => new Promise<Response>(() => {}) });
    render(page(matched));
    await open();
    await screen.findByText("Looking for this album's release…");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(20_500);
    });
    await screen.findByRole("heading", { name: "Showing only your tracks" });
    expect(screen.getByRole("alert").textContent).toContain("timed out");
    expect(screen.getByRole("table", { name: "Tracks" })).toBeTruthy();
  } finally {
    vi.useRealTimers();
  }
});

test("an album whose title is a remix is not lined up with the original release automatically", async () => {
  serve();
  const remix = {
    ...matched,
    release: {
      ...matched.release,
      title: `${editionGroup.title} (feat. Someone) - Remix`,
    },
  };
  render(page(remix));
  await open();
  await screen.findByRole("heading", {
    name: `Which release is ${remix.release.title}?`,
  });
  expect(screen.queryByText(/^Lined up with/)).toBeNull();
  expect(
    screen.getByRole("button", { name: "Choose the release" }),
  ).toBeTruthy();
});

test("closing the edition question with Escape keeps only the album's own tracks", async () => {
  serve({
    [editions]: () =>
      Response.json({
        ...firstEditionPage,
        continuation: null,
        page: {
          ...firstEditionPage.page,
          items: [...firstEditionPage.page.items, bonusEdition],
        },
      }),
  });
  render(page(matched));
  await open();
  const dialog = await screen.findByRole("dialog", {
    name: `Is this ${editionGroup.title} from 2024?`,
  });
  fireEvent(dialog, new Event("cancel", { cancelable: true }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(
    await screen.findByRole("button", { name: "Choose an edition" }),
  ).toBeTruthy();
});

test("picking a different release keeps the question in a dialog", async () => {
  serve({
    [editions]: () =>
      Response.json({
        ...firstEditionPage,
        continuation: null,
        page: {
          ...firstEditionPage.page,
          items: [...firstEditionPage.page.items, bonusEdition],
        },
      }),
  });
  render(page(matched));
  await open();
  const question = await screen.findByRole("dialog");
  fireEvent.click(
    within(question).getByRole("button", { name: "It's a different release" }),
  );
  const picker = await screen.findByRole("dialog", {
    name: `Which release is ${matched.release.title}?`,
  });
  expect(
    within(picker).getByRole("list", { name: "Releases to choose from" }),
  ).toBeTruthy();
});

test("closing Change edition with Escape returns focus to the edition menu", async () => {
  serve({
    [editions]: () =>
      Response.json({
        ...firstEditionPage,
        continuation: null,
        page: {
          ...firstEditionPage.page,
          items: [...firstEditionPage.page.items, bonusEdition],
        },
      }),
  });
  render(page(matched));
  await open();
  const question = await screen.findByRole("dialog", {
    name: `Is this ${editionGroup.title} from 2024?`,
  });
  await within(question).findAllByRole("radio");
  fireEvent.click(within(question).getByRole("button", { name: "Use this" }));
  await screen.findByRole("region", { name: "Album edition track list" });
  fireEvent.click(await screen.findByRole("button", { name: /^Edition / }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Change edition" }));
  const change = await screen.findByRole("dialog");
  fireEvent(change, new Event("cancel", { cancelable: true }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await waitFor(() =>
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: /^Edition / }),
    ),
  );
});
