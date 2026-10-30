import { createElement } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { unselectedArtistIdentity } from "../fixtures/artistIdentity";
import ArtistReview from "../components/ArtistReview";
import type { Schema } from "../../../shared/schema";
import {
  noAssociationChanges,
  artistAssociation,
  artistPreview,
  artistQuery,
  artistReviewAccount,
  artistReviewConnections,
  artistReviewContext,
  artistReviewDetail,
  artistReviewSettings,
  artistSearch,
  artistSearchResult,
  artistSelection,
  secondCatalog,
} from "../fixtures/artistReview";

test("an expired artist confirmation is discarded until an explicit fresh review is confirmed", async () => {
  const errors = vi.spyOn(console, "error");
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const before = structuredClone(artistReviewDetail);
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  const review = `${base}/artist-review`;
  const associations = `${base}/artist-associations`;
  const freshContext = {
    ...artistReviewContext,
    metadata_binding: {
      ...artistReviewContext.metadata_binding,
      revision: artistReviewContext.metadata_binding.revision + 1,
    },
  } satisfies Schema<"ArtistReviewContext">;
  const freshProfile = {
    ...artistPreview.profile,
    disambiguation: "Profile returned for the fresh artist review",
    revision: "private-provider-profile:12",
  } satisfies Schema<"CatalogArtistProfile">;
  const freshSelection = {
    context: freshContext,
    provider_artist_id: freshProfile.artist.id,
  } satisfies Schema<"SelectArtistCandidate">;
  const responses: Response[] = [];
  const reply = (value: unknown, status = 200) => {
    const response = Response.json(value, { status });
    responses.push(response);
    return response;
  };
  let contextReads = 0;
  let confirmations = 0;
  let persisted = false;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === `${base}/artist-association-changes`)
      return Response.json(noAssociationChanges);
    if (method === "GET" && path === `${base}/artist-identity`)
      return reply(unselectedArtistIdentity);
    if (method === "GET" && path === "/api/v1/metadata/settings")
      return reply(artistReviewSettings);
    if (method === "GET" && path === "/api/v1/plugins")
      return reply(artistReviewConnections);
    if (method === "GET" && path === associations)
      return reply(persisted ? [artistAssociation] : []);
    if (method === "POST" && path === `${review}/context`) {
      contextReads++;
      expect(contextReads).toBeLessThanOrEqual(2);
      return reply(contextReads === 1 ? artistReviewContext : freshContext);
    }
    if (method === "POST" && path === `${review}/search`)
      return reply({
        ...artistSearchResult,
        context: contextReads === 1 ? artistReviewContext : freshContext,
      } satisfies Schema<"ArtistCandidateSearchAvailability">);
    if (method === "POST" && path === `${review}/preview`)
      return reply(
        contextReads === 1
          ? artistPreview
          : ({
              status: "available",
              context: freshContext,
              profile: freshProfile,
            } satisfies Schema<"ArtistCandidatePreviewAvailability">),
      );
    if (method === "POST" && path === `${review}/confirm`) {
      confirmations++;
      if (confirmations === 1)
        return reply(
          { code: "conflict", error: "Artist review changed. Search again." },
          409,
        );
      expect(confirmations).toBe(2);
      expect(contextReads).toBe(2);
      expect(JSON.parse(options?.body as string)).toEqual(freshSelection);
      persisted = true;
      return reply(artistAssociation);
    }
    if (
      method === "GET" &&
      path === `${associations}/${artistAssociation.id}/profile` &&
      persisted
    )
      return reply({
        status: "available",
        association_id: artistAssociation.id,
        profile: freshProfile,
      } satisfies Schema<"ArtistProfileAvailability">);
    throw new Error(
      "Unexpected HTTP request outside the owned artist review recovery",
    );
  });
  vi.stubGlobal("fetch", fetch);
  const writes = () =>
    fetch.mock.calls.filter(([, options]) => options?.method === "POST");
  const view = render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: artistReviewDetail,
    }),
  );
  async function reviewSecondArtist() {
    const results = await screen.findByRole("list", {
      name: "Artist search results",
    });
    const rows = within(results).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    fireEvent.click(
      within(rows[1]).getByRole("button", { name: "Review artist" }),
    );
    return screen.findByRole("region", { name: "Review artist" });
  }
  try {
    fireEvent.click(screen.getByRole("button", { name: "Change artist" }));
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
    const expiredPreview = await reviewSecondArtist();
    expect(
      within(expiredPreview).getByText(artistPreview.profile.disambiguation, {
        exact: true,
      }),
    ).toBeTruthy();
    fireEvent.click(
      within(expiredPreview).getByRole("button", { name: "Use this artist" }),
    );
    expect((await screen.findByRole("alert")).textContent).toBe(
      "Artist review changed. Search again.",
    );
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.queryByRole("region", { name: "Review artist" })).toBeNull();
    expect(
      screen.queryByRole("list", { name: "Artist search results" }),
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
    expect(
      (
        screen.getByRole("searchbox", {
          name: "Search artists",
        }) as HTMLInputElement
      ).value,
    ).toBe(artistQuery);
    expect((provider as HTMLSelectElement).value).toBe(
      secondCatalog.installation.id,
    );
    expect(
      (
        screen.getByRole("button", {
          name: "Search artists",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false);
    expect(contextReads).toBe(1);
    expect(confirmations).toBe(1);
    expect(persisted).toBe(false);
    expect(fetch).toHaveBeenCalledTimes(9);
    expect(JSON.parse(writes()[3][1]?.body as string)).toEqual(artistSelection);
    expect(responses.every((response) => response.bodyUsed)).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Search artists" }));
    const freshPreview = await reviewSecondArtist();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(
      within(freshPreview).getByText(freshProfile.disambiguation, {
        exact: true,
      }),
    ).toBeTruthy();
    expect(contextReads).toBe(2);
    expect(confirmations).toBe(1);
    expect(persisted).toBe(false);
    expect(fetch).toHaveBeenCalledTimes(12);
    expect(JSON.parse(writes()[4][1]?.body as string)).toEqual({
      provider: freshContext.provider,
    });
    expect(JSON.parse(writes()[5][1]?.body as string)).toEqual({
      context: freshContext,
      search: artistSearch,
    } satisfies Schema<"SearchArtistCandidates">);
    expect(JSON.parse(writes()[6][1]?.body as string)).toEqual(freshSelection);
    fireEvent.click(
      within(freshPreview).getByRole("button", { name: "Use this artist" }),
    );
    await screen.findByText("Artist review saved.", { exact: true });
    const profile = await screen.findByRole("region", {
      name: "Reviewed artist profile",
    });
    expect(
      within(profile).getByText(freshProfile.disambiguation, { exact: true }),
    ).toBeTruthy();
    expect(confirmations).toBe(2);
    expect(persisted).toBe(true);
    expect(fetch.mock.calls.map(([path]) => path)).toEqual([
      "/api/v1/metadata/settings",
      "/api/v1/plugins",
      associations,
      `${base}/artist-identity`,
      `${base}/artist-association-changes`,
      `${review}/context`,
      `${review}/search`,
      `${review}/preview`,
      `${review}/confirm`,
      `${review}/context`,
      `${review}/search`,
      `${review}/preview`,
      `${review}/confirm`,
      `${associations}/${artistAssociation.id}/profile`,
    ]);
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
    view.unmount();
    expect(
      fetch.mock.calls.every(([, options]) => options?.signal?.aborted),
    ).toBe(true);
    expect(errors).not.toHaveBeenCalled();
  } finally {
    view.unmount();
  }
});
