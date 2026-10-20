// @vitest-environment jsdom
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
import { afterEach, expect, test, vi } from "vitest";
import ArtistReview from "./components/metadata/ArtistReview";
import type { components } from "./generated";
import { unselectedArtistIdentity } from "./fixtures/artistIdentity";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewConnections,
  artistReviewDetail,
  artistReviewSettings,
  reviewedArtistProfile,
} from "./fixtures/artistReview";
import {
  discographyCredit,
  discographyTitle,
  firstDiscographyPage,
  hostDiscographyContinuation,
  lastDiscographyPage,
  providerDiscographyCursor,
} from "./fixtures/artistDiscography";

type Schema = components["schemas"];

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  localStorage.clear();
  sessionStorage.clear();
});

test("a saved artist review browses paged catalog groups without adopting albums or preparing audio", async () => {
  const errors = vi.spyOn(console, "error");
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const push = vi.spyOn(window.history, "pushState");
  const replace = vi.spyOn(window.history, "replaceState");
  const before = structuredClone(artistReviewDetail);
  const associations = `/api/v1/recordings/${artistReviewDetail.recording_id}/artist-associations`;
  const profilePath = `${associations}/${artistAssociation.id}/profile`;
  const discographyPath = `${associations}/${artistAssociation.id}/discography`;
  const responses: Response[] = [];
  const reply = (value: unknown) => {
    const response = Response.json(value);
    responses.push(response);
    return response;
  };
  let releaseMore!: (response: Response) => void;
  const more = new Promise<Response>((resolve) => {
    releaseMore = resolve;
  });
  let discographyReads = 0;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (
      method === "GET" &&
      path ===
        `/api/v1/recordings/${artistReviewDetail.recording_id}/artist-identity`
    )
      return reply(unselectedArtistIdentity);
    if (method === "GET" && path === "/api/v1/metadata/settings")
      return reply(artistReviewSettings);
    if (method === "GET" && path === "/api/v1/plugins")
      return reply(artistReviewConnections);
    if (method === "GET" && path === associations)
      return reply([artistAssociation]);
    if (method === "GET" && path === profilePath)
      return reply(reviewedArtistProfile);
    if (method === "POST" && path === discographyPath) {
      discographyReads++;
      if (discographyReads === 1) return reply(firstDiscographyPage);
      if (discographyReads === 2) return more;
    }
    throw new Error(
      "Unexpected HTTP request outside read-only artist discography browsing",
    );
  });
  vi.stubGlobal("fetch", fetch);
  const posts = () =>
    fetch.mock.calls.filter(([, options]) => options?.method === "POST");
  const view = render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: artistReviewDetail,
    }),
  );
  try {
    expect(fetch).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "Find artist information" }),
    );
    const saved = await screen.findByRole("button", {
      name: "View saved artist review 1",
    });
    expect(fetch.mock.calls.map(([path]) => path).sort()).toEqual(
      [
        "/api/v1/metadata/settings",
        "/api/v1/plugins",
        associations,
        `/api/v1/recordings/${artistReviewDetail.recording_id}/artist-identity`,
      ].sort(),
    );
    expect(posts()).toHaveLength(0);
    fireEvent.click(saved);
    const profile = await screen.findByRole("region", {
      name: "Reviewed artist profile",
    });
    expect(
      within(profile).getByRole("heading", {
        name: reviewedArtistProfile.profile.artist.name,
      }),
    ).toBeTruthy();
    await act(async () => {
      await Promise.resolve();
    });
    expect(fetch).toHaveBeenCalledTimes(5);
    expect(posts()).toHaveLength(0);
    fireEvent.click(
      within(profile).getByRole("button", { name: "Browse discography" }),
    );
    const region = await screen.findByRole("region", { name: "Discography" });
    const list = await within(region).findByRole("list", {
      name: "Discography releases",
    });
    await waitFor(() =>
      expect(within(list).getAllByRole("listitem")).toHaveLength(1),
    );
    const initialRows = within(list).getAllByRole("listitem");
    expect(
      within(initialRows[0]).getByRole("heading", { name: discographyTitle }),
    ).toBeTruthy();
    expect(initialRows[0].textContent).toContain(discographyCredit);
    expect(
      within(initialRows[0]).getByText("Album", { exact: true }),
    ).toBeTruthy();
    expect(
      within(initialRows[0]).getByText("Live", { exact: true }),
    ).toBeTruthy();
    expect(
      within(initialRows[0]).getByText("2024-02", { exact: true }),
    ).toBeTruthy();
    expect(posts()).toHaveLength(1);
    expect(posts()[0][0]).toBe(discographyPath);
    expect(JSON.parse(posts()[0][1]?.body as string)).toEqual({
      limit: 20,
      continuation: null,
    } satisfies Schema["ReadArtistDiscography"]);

    fireEvent.click(
      within(region).getByRole("button", { name: "Load more releases" }),
    );
    await waitFor(() => expect(posts()).toHaveLength(2));
    expect(region.getAttribute("aria-busy")).toBe("true");
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    expect(
      within(list).getByRole("heading", { name: discographyTitle }),
    ).toBeTruthy();
    expect(JSON.parse(posts()[1][1]?.body as string)).toEqual({
      limit: 20,
      continuation: hostDiscographyContinuation,
    } satisfies Schema["ReadArtistDiscography"]);
    expect(posts()[1][1]?.body).not.toContain(providerDiscographyCursor);
    await act(async () => {
      releaseMore(reply(lastDiscographyPage));
    });
    await waitFor(() =>
      expect(within(list).getAllByRole("listitem")).toHaveLength(2),
    );
    const rows = within(list).getAllByRole("listitem");
    for (const row of rows)
      expect(
        within(row).getByRole("heading", { name: discographyTitle }),
      ).toBeTruthy();
    expect(rows[0].textContent).toContain(discographyCredit);
    expect(within(rows[0]).getByText("2024-02", { exact: true })).toBeTruthy();
    expect(
      within(rows[1]).getByText("Type not provided", { exact: true }),
    ).toBeTruthy();
    expect(
      within(rows[1]).getByText("Artist credit not provided", { exact: true }),
    ).toBeTruthy();
    expect(
      within(rows[1]).getByText("Release date not provided", { exact: true }),
    ).toBeTruthy();
    expect(region.getAttribute("aria-busy")).not.toBe("true");
    expect(
      within(region).queryByRole("button", { name: "Load more releases" }),
    ).toBeNull();
    expect(within(region).queryAllByRole("link")).toHaveLength(0);
    expect(within(region).queryAllByRole("img")).toHaveLength(0);
    expect(
      within(region).queryByRole("button", {
        name: /play|download|import|add to library/i,
      }),
    ).toBeNull();
    expect(fetch.mock.calls.map(([path]) => path)).toEqual([
      "/api/v1/metadata/settings",
      "/api/v1/plugins",
      associations,
      `/api/v1/recordings/${artistReviewDetail.recording_id}/artist-identity`,
      profilePath,
      discographyPath,
      discographyPath,
    ]);
    for (const [, options] of fetch.mock.calls) {
      const headers = new Headers(options?.headers);
      expect(options?.credentials).toBe("same-origin");
      expect(options?.signal).toBeInstanceOf(AbortSignal);
      expect(headers.get("x-hito-account")).toBe(artistReviewAccount);
      expect(headers.get("x-hito-request")).toBe("1");
      expect(headers.get("content-type")).toBe("application/json");
    }
    expect(responses.every((response) => response.bodyUsed)).toBe(true);
    expect(artistReviewDetail).toEqual(before);
    expect(play).not.toHaveBeenCalled();
    expect(push).not.toHaveBeenCalled();
    expect(replace).not.toHaveBeenCalled();
    expect(errors).not.toHaveBeenCalled();
  } finally {
    releaseMore(Response.json(lastDiscographyPage));
    view.unmount();
  }
});
