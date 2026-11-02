import { createElement, type AnchorHTMLAttributes } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ArtistReview from "../components/ArtistReview";
import { artistSearchHref, opensArtistSearch } from "../model/artistSearchLink";
import {
  artistReviewAccount,
  artistReviewConnections,
  artistReviewDetail,
  artistReviewSettings,
} from "../fixtures/artistReview";
import { unselectedArtistIdentity } from "../fixtures/artistIdentity";

vi.mock("@inertiajs/react", () => ({
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

test("the search link and the page agree on which query opens the artist search", () => {
  const href = artistSearchHref("rec 1");
  expect(href).toBe("/recordings/rec%201?review=artist");
  expect(opensArtistSearch(href.slice(href.indexOf("?")))).toBe(true);
  expect(opensArtistSearch("")).toBe(false);
  expect(opensArtistSearch("?review=other")).toBe(false);
});

test("opening a recording from the review queue shows the artist search with the credit name filled in", async () => {
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  const calls: string[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      calls.push(`${options?.method ?? "GET"} ${path}`);
      if (path === "/api/v1/metadata/settings")
        return Response.json(artistReviewSettings);
      if (path === "/api/v1/plugins")
        return Response.json(artistReviewConnections);
      if (path === `${base}/artist-associations`) return Response.json([]);
      if (path === `${base}/artist-identity`)
        return Response.json(unselectedArtistIdentity);
      throw new Error(`Unexpected ${path}`);
    }),
  );
  render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: structuredClone(artistReviewDetail),
      openSearch: true,
    }),
  );
  await screen.findByRole("heading", { name: "Choose the artist" });
  const field = (await screen.findByLabelText(
    "Search artists",
  )) as HTMLInputElement;
  expect(field.value).toBe(artistReviewDetail.artist.value);
  expect(calls.filter((call) => !call.startsWith("GET "))).toEqual([]);
  expect(calls.some((call) => call.includes("artist-review"))).toBe(false);
});

test("a failed automatic open can be retried with Change artist", async () => {
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  let settings = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string) => {
      if (path === "/api/v1/metadata/settings")
        return ++settings === 1
          ? new Response("{}", { status: 500 })
          : Response.json(artistReviewSettings);
      if (path === "/api/v1/plugins")
        return Response.json(artistReviewConnections);
      if (path === `${base}/artist-associations`) return Response.json([]);
      if (path === `${base}/artist-identity`)
        return Response.json(unselectedArtistIdentity);
      throw new Error(`Unexpected ${path}`);
    }),
  );
  render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: structuredClone(artistReviewDetail),
      openSearch: true,
    }),
  );
  await screen.findByRole("alert");
  expect(
    screen.queryByRole("heading", { name: "Choose the artist" }),
  ).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Change artist" }));
  await screen.findByRole("heading", { name: "Choose the artist" });
});
