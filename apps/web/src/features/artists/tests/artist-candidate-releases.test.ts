import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { unselectedArtistIdentity } from "../fixtures/artistIdentity";
import ArtistReview from "../components/ArtistReview";
import {
  artistReviewAccount,
  artistReviewConnections,
  artistReviewContext,
  artistReviewDetail,
  artistReviewSettings,
  artistSearchResult,
  noAssociationChanges,
} from "../fixtures/artistReview";

const FIRST = "0f800000-0000-4000-8000-0000000000a1";
const SECOND = "0f800000-0000-4000-8000-0000000000a2";
const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
const review = `${base}/artist-review`;

const builtinContext = {
  ...artistReviewContext,
  provider: { kind: "builtin_musicbrainz" },
};

const builtinSearch = {
  ...artistSearchResult,
  context: builtinContext,
  page: {
    ...artistSearchResult.page,
    items: [
      { artist: { id: FIRST, name: "Adele" }, kind: "person" },
      { artist: { id: SECOND, name: "Adele" }, kind: "group" },
    ],
  },
};

const watched: { element: Element; fire: () => void }[] = [];

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  watched.length = 0;
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      constructor(private callback: IntersectionObserverCallback) {}
      observe(element: Element) {
        watched.push({
          element,
          fire: () =>
            this.callback(
              [{ isIntersecting: true } as IntersectionObserverEntry],
              this as unknown as IntersectionObserver,
            ),
        });
      }
      disconnect() {}
    },
  );
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function releasesCalls(fetch: ReturnType<typeof vi.fn>) {
  return fetch.mock.calls.filter(([path]) => path === `${review}/releases`);
}

async function candidates(answer: (artist: string) => unknown) {
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (path === `${base}/artist-association-changes`)
      return Response.json(noAssociationChanges);
    if (path === `${base}/artist-identity`)
      return Response.json(unselectedArtistIdentity);
    if (path === "/api/v1/metadata/settings")
      return Response.json(artistReviewSettings);
    if (path === "/api/v1/plugins")
      return Response.json(artistReviewConnections);
    if (path === `${base}/artist-associations`) return Response.json([]);
    if (method === "POST" && path === `${review}/context`)
      return Response.json(builtinContext);
    if (method === "POST" && path === `${review}/search`)
      return Response.json(builtinSearch);
    if (method === "POST" && path === `${review}/releases`)
      return Response.json(
        answer(JSON.parse(options?.body as string).provider_artist_id),
      );
    throw new Error(`Unexpected request ${path}`);
  });
  vi.stubGlobal("fetch", fetch);
  render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: artistReviewDetail,
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Change artist" }));
  await screen.findByRole("combobox", { name: "Metadata provider" });
  fireEvent.click(screen.getByRole("button", { name: "Search artists" }));
  const list = await screen.findByRole("list", {
    name: "Artist search results",
  });
  return { fetch, rows: within(list).getAllByRole("listitem") };
}

async function scrollIntoView(row: HTMLElement) {
  await waitFor(() =>
    expect(watched.some(({ element }) => row.contains(element))).toBe(true),
  );
  const entry = watched.find(({ element }) => row.contains(element))!;
  act(() => entry.fire());
}

test("searching asks for no release titles and offers no button until a candidate scrolls into view", async () => {
  const { fetch, rows } = await candidates(() => ({ status: "unavailable" }));
  expect(rows).toHaveLength(2);
  expect(releasesCalls(fetch)).toHaveLength(0);
  expect(
    within(rows[0]).queryByRole("button", { name: "Show known releases" }),
  ).toBeNull();
});

test("a candidate that scrolls into view asks only for its artist and lists at most three release titles", async () => {
  const { fetch, rows } = await candidates((artist) => ({
    status: "available",
    context: builtinContext,
    titles:
      artist === FIRST
        ? ["Morning Light", "Second Wind", "Harbor Lights", "Fourth Tide"]
        : [],
  }));
  await scrollIntoView(rows[0]);
  expect(await within(rows[0]).findByText("Morning Light")).toBeTruthy();
  expect(within(rows[0]).getByText("Second Wind")).toBeTruthy();
  expect(within(rows[0]).getByText("Harbor Lights")).toBeTruthy();
  expect(within(rows[0]).queryByText("Fourth Tide")).toBeNull();
  const calls = releasesCalls(fetch);
  expect(calls).toHaveLength(1);
  expect(JSON.parse(calls[0][1]?.body as string)).toEqual({
    context: builtinContext,
    provider_artist_id: FIRST,
  });
  expect(within(rows[1]).queryByText("Morning Light")).toBeNull();
});

test("a candidate with no listed releases says so and a failed request can be retried", async () => {
  let answers = 0;
  const { fetch, rows } = await candidates(() =>
    answers++ === 0
      ? { status: "unavailable" }
      : { status: "available", context: builtinContext, titles: [] },
  );
  await scrollIntoView(rows[0]);
  expect(
    await within(rows[0]).findByText("Releases are unavailable. Try again."),
  ).toBeTruthy();
  fireEvent.click(within(rows[0]).getByRole("button", { name: "Try again" }));
  expect(
    await within(rows[0]).findByText("No releases are listed for this artist."),
  ).toBeTruthy();
  await waitFor(() => expect(releasesCalls(fetch)).toHaveLength(2));
});
