import { createElement } from "react";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { unselectedArtistIdentity } from "../fixtures/artistIdentity";
import ArtistReview from "../components/ArtistReview";
import type { components } from "../../../generated";
import {
  artistQuery,
  artistReviewAccount,
  artistReviewConnections,
  artistReviewContext,
  artistReviewDetail,
  artistReviewSettings,
  artistSearch,
  artistSearchResult,
  secondCatalog,
} from "../fixtures/artistReview";

type Schema = components["schemas"];

test.each(["credited artist", "same-name artist operation"] as const)(
  "a changed server %s requires refreshed track details before artist candidates are requested",
  async (change) => {
    const errors = vi.spyOn(console, "error");
    const play = vi
      .spyOn(HTMLMediaElement.prototype, "play")
      .mockResolvedValue();
    const before = structuredClone(artistReviewDetail);
    const detail = structuredClone(artistReviewDetail);
    detail.artist.operation_id = "80000000-0000-4000-8000-000000000004";
    if (change === "credited artist") {
      detail.artist.value = "Changed artist credit from another device";
      detail.artist.personal = detail.artist.value;
    }
    const context = {
      ...artistReviewContext,
      artist_operation_id: detail.artist.operation_id,
      credited_name: detail.artist.value,
    } satisfies Schema["ArtistReviewContext"];
    expect(context.account_id).toBe(artistReviewAccount);
    expect(context.recording_id).toBe(artistReviewDetail.recording_id);
    expect(context.observation_id).toBe(
      artistReviewDetail.selected_observation?.id,
    );
    expect(context.source_id).toBe(
      artistReviewDetail.selected_observation?.source_id,
    );
    const base = `/api/v1/recordings/${detail.recording_id}`;
    const review = `${base}/artist-review`;
    const associations = `${base}/artist-associations`;
    const responses: Response[] = [];
    const reply = (value: unknown) => {
      const response = Response.json(value);
      responses.push(response);
      return response;
    };
    const fetch = vi.fn(async (path: string, options?: RequestInit) => {
      const method = options?.method ?? "GET";
      if (method === "GET" && path === `${base}/artist-identity`)
        return reply(unselectedArtistIdentity);
      if (method === "GET" && path === "/api/v1/metadata/settings")
        return reply(artistReviewSettings);
      if (method === "GET" && path === "/api/v1/plugins")
        return reply(artistReviewConnections);
      if (method === "GET" && path === associations) return reply([]);
      if (method === "POST" && path === `${review}/context`)
        return reply(context);
      if (method === "POST" && path === `${review}/search`)
        return reply({
          ...artistSearchResult,
          context,
        } satisfies Schema["ArtistCandidateSearchAvailability"]);
      throw new Error(
        "Unexpected HTTP request outside the owned rendered-credit review",
      );
    });
    vi.stubGlobal("fetch", fetch);
    const view = render(
      createElement(ArtistReview, {
        account: artistReviewAccount,
        detail: artistReviewDetail,
      }),
    );
    async function explicitSearch() {
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
    }
    try {
      await explicitSearch();
      await waitFor(() =>
        expect(
          screen.queryByRole("alert") ??
            screen.queryByRole("list", { name: "Artist search results" }),
        ).not.toBeNull(),
      );
      expect(fetch.mock.calls.map(([path]) => path)).toEqual([
        "/api/v1/metadata/settings",
        "/api/v1/plugins",
        associations,
        `${base}/artist-identity`,
        `${review}/context`,
      ]);
      const alert = screen.getByRole("alert");
      expect(alert.textContent).toMatch(/artist.*changed|changed.*artist/i);
      expect(alert.textContent).toMatch(/refresh.*track/i);
      expect(
        screen.queryByRole("list", { name: "Artist search results" }),
      ).toBeNull();
      expect(
        screen.queryByRole("region", { name: "Review artist" }),
      ).toBeNull();
      expect(
        screen.queryByRole("button", { name: "Use this artist" }),
      ).toBeNull();
      expect(
        screen.queryByRole("region", { name: "Saved artist reviews" }),
      ).toBeNull();
      expect(
        screen.queryByRole("region", { name: "Reviewed artist profile" }),
      ).toBeNull();
      expect(
        screen.queryByText("Artist review saved.", { exact: true }),
      ).toBeNull();
      expect(responses.every((response) => response.bodyUsed)).toBe(true);
      const retired = [...fetch.mock.calls];

      view.rerender(
        createElement(ArtistReview, { account: artistReviewAccount, detail }),
      );
      expect(retired.every(([, options]) => options?.signal?.aborted)).toBe(
        true,
      );
      expect(fetch).toHaveBeenCalledTimes(5);
      expect(screen.queryByRole("alert")).toBeNull();
      expect(
        screen.getByRole("button", { name: "Find artist information" }),
      ).toBeTruthy();
      expect(
        screen.queryByRole("combobox", { name: "Metadata provider" }),
      ).toBeNull();
      await explicitSearch();
      const results = await screen.findByRole("list", {
        name: "Artist search results",
      });
      expect(within(results).getAllByRole("listitem")).toHaveLength(2);
      expect(screen.queryByRole("alert")).toBeNull();
      const currentCalls = fetch.mock.calls.slice(retired.length);
      expect(currentCalls.map(([path]) => path)).toEqual([
        "/api/v1/metadata/settings",
        "/api/v1/plugins",
        associations,
        `${base}/artist-identity`,
        `${review}/context`,
        `${review}/search`,
      ]);
      expect(JSON.parse(currentCalls[4][1]?.body as string)).toEqual({
        provider: context.provider,
      });
      expect(JSON.parse(currentCalls[5][1]?.body as string)).toEqual({
        context,
        search: artistSearch,
      } satisfies Schema["SearchArtistCandidates"]);
      for (const [, options] of fetch.mock.calls) {
        expect(new Headers(options?.headers).get("x-hito-account")).toBe(
          artistReviewAccount,
        );
        expect(options?.credentials).toBe("same-origin");
        expect(options?.signal).toBeInstanceOf(AbortSignal);
      }
      expect(responses.every((response) => response.bodyUsed)).toBe(true);
      expect(artistReviewDetail).toEqual(before);
      expect(context.source_id).toBeNull();
      expect(play).not.toHaveBeenCalled();
      expect(document.querySelectorAll("audio")).toHaveLength(0);
      expect(localStorage.length + sessionStorage.length).toBe(0);
      view.unmount();
      expect(
        fetch.mock.calls.every(([, options]) => options?.signal?.aborted),
      ).toBe(true);
      expect(errors).not.toHaveBeenCalled();
    } finally {
      view.unmount();
    }
  },
);
