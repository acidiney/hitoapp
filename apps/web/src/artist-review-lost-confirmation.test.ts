// @vitest-environment jsdom
import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { unselectedArtistIdentity } from "./fixtures/artistIdentity";
import ArtistReview from "./components/metadata/ArtistReview";
import {
  artistAssociation,
  artistPreview,
  artistQuery,
  artistReviewAccount,
  artistReviewConnections,
  artistReviewContext,
  artistReviewDetail,
  artistReviewSettings,
  artistSearchResult,
  artistSelection,
  reviewedArtistProfile,
  secondCatalog,
} from "./fixtures/artistReview";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  localStorage.clear();
  sessionStorage.clear();
});

test("a lost confirmation acknowledgement is recovered by an explicit authoritative read after returning to the track", async () => {
  const errors = vi.spyOn(console, "error");
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const before = structuredClone(artistReviewDetail);
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  const review = `${base}/artist-review`;
  const associations = `${base}/artist-associations`;
  const profilePath = `${associations}/${artistAssociation.id}/profile`;
  const responses: Response[] = [];
  const reply = (value: unknown) => {
    const response = Response.json(value);
    responses.push(response);
    return response;
  };
  let committed = false;
  let confirmations = 0;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === `${base}/artist-identity`)
      return reply(unselectedArtistIdentity);
    if (method === "GET" && path === "/api/v1/metadata/settings")
      return reply(artistReviewSettings);
    if (method === "GET" && path === "/api/v1/plugins")
      return reply(artistReviewConnections);
    if (method === "GET" && path === associations)
      return reply(committed ? [artistAssociation] : []);
    if (method === "POST" && path === `${review}/context`)
      return reply(artistReviewContext);
    if (method === "POST" && path === `${review}/search`)
      return reply(artistSearchResult);
    if (method === "POST" && path === `${review}/preview`)
      return reply(artistPreview);
    if (method === "POST" && path === `${review}/confirm`) {
      confirmations++;
      expect(confirmations).toBe(1);
      expect(JSON.parse(options?.body as string)).toEqual(artistSelection);
      committed = true;
      throw new TypeError("The confirmation response was lost.");
    }
    if (method === "GET" && path === profilePath && committed)
      return reply(reviewedArtistProfile);
    throw new Error(
      "Unexpected HTTP request outside the owned lost confirmation recovery",
    );
  });
  vi.stubGlobal("fetch", fetch);
  const component = () =>
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: artistReviewDetail,
    });
  const initial = render(component());
  let returned: ReturnType<typeof render> | null = null;
  try {
    fireEvent.click(
      screen.getByRole("button", { name: "Find artist information" }),
    );
    const provider = await screen.findByRole("combobox", {
      name: "Metadata provider",
    });
    fireEvent.change(provider, {
      target: { value: secondCatalog.installation.id },
    });
    fireEvent.change(
      screen.getByRole("searchbox", { name: "Search artists" }),
      { target: { value: artistQuery } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Search artists" }));
    const candidates = await screen.findByRole("list", {
      name: "Artist search results",
    });
    const rows = within(candidates).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    fireEvent.click(
      within(rows[1]).getByRole("button", { name: "Review artist" }),
    );
    const preview = await screen.findByRole("region", {
      name: "Review artist",
    });
    fireEvent.click(
      within(preview).getByRole("button", { name: "Use this artist" }),
    );
    expect((await screen.findByRole("alert")).textContent).toBe(
      "The confirmation response was lost.",
    );
    await act(async () => {
      await Promise.resolve();
    });
    expect(committed).toBe(true);
    expect(confirmations).toBe(1);
    expect(fetch).toHaveBeenCalledTimes(8);
    expect(
      screen.queryByText("Artist review saved.", { exact: true }),
    ).toBeNull();
    expect(
      screen.queryByRole("region", { name: "Saved artist reviews" }),
    ).toBeNull();
    expect(
      screen.queryByRole("region", { name: "Reviewed artist profile" }),
    ).toBeNull();
    expect(screen.queryByRole("region", { name: "Review artist" })).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Use this artist" }),
    ).toBeNull();
    expect(responses.every((response) => response.bodyUsed)).toBe(true);
    const retired = [...fetch.mock.calls];
    initial.unmount();
    expect(retired.every(([, options]) => options?.signal?.aborted)).toBe(true);

    returned = render(component());
    expect(fetch).toHaveBeenCalledTimes(8);
    expect(
      screen.getByRole("button", { name: "Find artist information" }),
    ).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(
      screen.queryByRole("region", { name: "Saved artist reviews" }),
    ).toBeNull();
    expect(
      screen.queryByRole("region", { name: "Reviewed artist profile" }),
    ).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "Find artist information" }),
    );
    const saved = await screen.findByRole("region", {
      name: "Saved artist reviews",
    });
    expect(within(saved).getAllByRole("listitem")).toHaveLength(1);
    expect(confirmations).toBe(1);
    expect(fetch).toHaveBeenCalledTimes(12);
    expect(fetch.mock.calls.some(([path]) => path === profilePath)).toBe(false);
    expect(
      screen.queryByRole("region", { name: "Reviewed artist profile" }),
    ).toBeNull();
    expect(
      screen.queryByText("Artist review saved.", { exact: true }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Use this artist" }),
    ).toBeNull();
    fireEvent.click(
      within(saved).getByRole("button", { name: "View saved artist review 1" }),
    );
    const profile = await screen.findByRole("region", {
      name: "Reviewed artist profile",
    });
    expect(
      within(profile).getByText(artistPreview.profile.artist.name, {
        exact: true,
      }),
    ).toBeTruthy();
    expect(
      within(profile).getByText(artistPreview.profile.disambiguation, {
        exact: true,
      }),
    ).toBeTruthy();
    expect(confirmations).toBe(1);
    expect(fetch.mock.calls.map(([path]) => path)).toEqual([
      "/api/v1/metadata/settings",
      "/api/v1/plugins",
      associations,
      `${base}/artist-identity`,
      `${review}/context`,
      `${review}/search`,
      `${review}/preview`,
      `${review}/confirm`,
      "/api/v1/metadata/settings",
      "/api/v1/plugins",
      associations,
      `${base}/artist-identity`,
      profilePath,
    ]);
    expect(
      fetch.mock.calls.filter(([, options]) => options?.method === "POST"),
    ).toHaveLength(4);
    expect(responses).toHaveLength(12);
    expect(responses.every((response) => response.bodyUsed)).toBe(true);
    for (const [, options] of fetch.mock.calls) {
      expect(new Headers(options?.headers).get("x-hito-account")).toBe(
        artistReviewAccount,
      );
      expect(options?.credentials).toBe("same-origin");
      expect(options?.signal).toBeInstanceOf(AbortSignal);
    }
    expect(artistReviewDetail).toEqual(before);
    expect(artistAssociation.source_id).toBeNull();
    expect(document.querySelectorAll("audio")).toHaveLength(0);
    expect(play).not.toHaveBeenCalled();
    expect(localStorage.length + sessionStorage.length).toBe(0);
    returned.unmount();
    expect(
      fetch.mock.calls.every(([, options]) => options?.signal?.aborted),
    ).toBe(true);
    expect(errors).not.toHaveBeenCalled();
  } finally {
    initial.unmount();
    returned?.unmount();
  }
});
