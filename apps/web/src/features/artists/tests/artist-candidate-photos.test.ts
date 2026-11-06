import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { unselectedArtistIdentity } from "../fixtures/artistIdentity";
import ArtistReview from "../components/ArtistReview";
import ArtistPhoto from "../components/ArtistPhoto";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewConnections,
  artistReviewContext,
  artistReviewDetail,
  artistReviewSettings,
  artistSearchResult,
  noAssociationChanges,
  secondCatalog,
} from "../fixtures/artistReview";

const FIRST = "0f800000-0000-4000-8000-0000000000a1";
const SECOND = "0f800000-0000-4000-8000-0000000000a2";

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
      {
        artist: { id: FIRST, name: "Adele" },
        kind: "person",
        disambiguation: "UK soul/jazz singer",
        country: "GB",
        begin_year: 2008,
      },
      {
        artist: { id: SECOND, name: "Adele" },
        kind: "group",
        disambiguation: null,
      },
    ],
  },
};

beforeEach(() => vi.useFakeTimers({ shouldAdvanceTime: true }));
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

async function search(provider?: string) {
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  const review = `${base}/artist-review`;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
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
        return Response.json(provider ? artistReviewContext : builtinContext);
      if (method === "POST" && path === `${review}/search`)
        return Response.json(
          provider
            ? { ...builtinSearch, context: artistReviewContext }
            : builtinSearch,
        );
      throw new Error(`Unexpected request ${path}`);
    }),
  );
  render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: artistReviewDetail,
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Change artist" }));
  const select = (await screen.findByRole("combobox", {
    name: "Metadata provider",
  })) as HTMLSelectElement;
  if (provider) fireEvent.change(select, { target: { value: provider } });
  fireEvent.click(screen.getByRole("button", { name: "Search artists" }));
  const list = await screen.findByRole("list", {
    name: "Artist search results",
  });
  return within(list).getAllByRole("listitem");
}

test("each built-in candidate shows a round photo from the same-origin route with its country and start year beside it", async () => {
  const rows = await search();
  const first = within(rows[0]);
  expect(first.getByText("Person · United Kingdom · 2008–")).toBeTruthy();
  expect(first.getByText("UK soul/jazz singer")).toBeTruthy();
  const image = rows[0].querySelector("img");
  expect(image?.getAttribute("src")).toBe(`/api/v1/cover-art/artist/${FIRST}`);
  expect(
    first.getByRole("img", { name: "Photo of Adele", hidden: true }),
  ).toBeTruthy();
  expect(within(rows[1]).getByText("Group")).toBeTruthy();
  expect(rows[1].querySelector("img")?.getAttribute("src")).toBe(
    `/api/v1/cover-art/artist/${SECOND}`,
  );
  expect(first.getByRole("button", { name: "Review artist" })).toBeTruthy();
});

test("a candidate from a personal provider never asks the photo route", async () => {
  const rows = await search(secondCatalog.installation.id);
  expect(rows[0].querySelector("img")).toBeNull();
  expect(
    within(rows[0]).getByRole("img", { name: "Photo of Adele", hidden: true }),
  ).toBeTruthy();
  expect(artistAssociation.id).toBeTruthy();
});

test("a photo that never arrives leaves the initial circle and is retried a bounded number of times", async () => {
  render(
    createElement(ArtistPhoto, {
      artist: FIRST,
      identity: FIRST,
      name: "Adele",
    }),
  );
  const sources = () => document.querySelectorAll("img").length;
  expect(sources()).toBe(1);
  for (let attempt = 0; attempt < 6; attempt++) {
    const image = document.querySelector("img");
    if (image) fireEvent.error(image);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
  }
  expect(document.querySelector("img")).toBeNull();
  expect(
    screen.getByRole("img", { name: "Photo of Adele", hidden: true }),
  ).toBeTruthy();
});

test("a photo that arrives on a retry replaces the circle", async () => {
  render(
    createElement(ArtistPhoto, {
      artist: FIRST,
      identity: FIRST,
      name: "Adele",
    }),
  );
  fireEvent.error(document.querySelector("img")!);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10_000);
  });
  const retried = document.querySelector("img")!;
  fireEvent.load(retried);
  expect(document.querySelector(".artwork")?.hasAttribute("data-loaded")).toBe(
    true,
  );
});

test("an exhausted photo shows the supplied fallback instead", async () => {
  render(
    createElement(
      ArtistPhoto,
      { artist: FIRST, identity: FIRST, name: "Adele" },
      createElement("p", null, "Album cover instead"),
    ),
  );
  for (let attempt = 0; attempt < 6; attempt++) {
    const image = document.querySelector("img");
    if (image) fireEvent.error(image);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
  }
  expect(screen.getByText("Album cover instead")).toBeTruthy();
});
