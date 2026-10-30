import { createElement } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
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
  firstCatalog,
  reviewedArtistProfile,
  secondCatalog,
} from "../fixtures/artistReview";

test("artist information is searched and previewed before its exact selected identity is explicitly confirmed", async () => {
  const errors = vi.spyOn(console, "error");
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const before = structuredClone(artistReviewDetail);
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  const review = `${base}/artist-review`;
  const associations = `${base}/artist-associations`;
  let persisted = false;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === `${base}/artist-association-changes`)
      return Response.json(noAssociationChanges);
    if (method === "GET" && path === `${base}/artist-identity`)
      return Response.json(unselectedArtistIdentity);
    if (method === "GET" && path === "/api/v1/metadata/settings")
      return Response.json(artistReviewSettings);
    if (method === "GET" && path === "/api/v1/plugins")
      return Response.json(artistReviewConnections);
    if (method === "GET" && path === associations)
      return Response.json(persisted ? [artistAssociation] : []);
    if (method === "POST" && path === `${review}/context`)
      return Response.json(artistReviewContext);
    if (method === "POST" && path === `${review}/search`)
      return Response.json(artistSearchResult);
    if (method === "POST" && path === `${review}/preview`)
      return Response.json(artistPreview);
    if (method === "POST" && path === `${review}/confirm`) {
      persisted = true;
      return Response.json(artistAssociation);
    }
    if (
      method === "GET" &&
      path === `${associations}/${artistAssociation.id}/profile` &&
      persisted
    )
      return Response.json(reviewedArtistProfile);
    throw new Error(
      "Unexpected HTTP request outside the owned artist review workflow",
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
  expect(fetch).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: "Use this artist" })).toBeNull();

  fireEvent.click(
    screen.getByRole("button", {
      name: "Change artist",
    }),
  );
  const provider = (await screen.findByRole("combobox", {
    name: "Metadata provider",
  })) as HTMLSelectElement;
  expect(provider.selectedOptions[0].textContent).toBe("MusicBrainz");
  expect(
    within(provider).getAllByRole("option", {
      name: firstCatalog.service.name,
    }),
  ).toHaveLength(2);
  expect(Array.from(provider.options, (option) => option.textContent)).toEqual([
    "MusicBrainz",
    firstCatalog.service.name,
    secondCatalog.service.name,
  ]);
  expect(fetch.mock.calls.map(([path]) => path).sort()).toEqual(
    [
      "/api/v1/metadata/settings",
      "/api/v1/plugins",
      associations,
      `${base}/artist-identity`,
      `${base}/artist-association-changes`,
    ].sort(),
  );
  fireEvent.change(provider, {
    target: { value: secondCatalog.installation.id },
  });
  const query = screen.getByRole("searchbox", {
    name: "Search artists",
  }) as HTMLInputElement;
  expect(query.value).toBe(artistReviewDetail.artist.value);
  fireEvent.change(query, { target: { value: artistQuery } });
  expect(fetch).toHaveBeenCalledTimes(5);
  expect(persisted).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "Search artists" }));
  const results = await screen.findByRole("list", {
    name: "Artist search results",
  });
  const rows = within(results).getAllByRole("listitem");
  expect(rows).toHaveLength(2);
  for (const row of rows)
    expect(
      within(row).getByRole("heading", {
        name: "Shared artist name",
      }),
    ).toBeTruthy();
  expect(within(rows[0]).getByText(/group/i)).toBeTruthy();
  expect(
    within(rows[0]).getByText(artistSearchResult.page.items[0].disambiguation, {
      exact: true,
    }),
  ).toBeTruthy();
  expect(within(rows[1]).getByText(/person/i)).toBeTruthy();
  expect(
    within(rows[1]).getByText(artistSearchResult.page.items[1].disambiguation, {
      exact: true,
    }),
  ).toBeTruthy();
  expect(writes().map(([path]) => path)).toEqual([
    `${review}/context`,
    `${review}/search`,
  ]);
  expect(JSON.parse(writes()[0][1]?.body as string)).toEqual({
    provider: artistReviewContext.provider,
  } satisfies Schema<"BeginArtistReview">);
  expect(JSON.parse(writes()[1][1]?.body as string)).toEqual({
    context: artistReviewContext,
    search: artistSearch,
  } satisfies Schema<"SearchArtistCandidates">);
  expect(persisted).toBe(false);
  expect(screen.queryByRole("button", { name: "Use this artist" })).toBeNull();

  fireEvent.click(
    within(rows[1]).getByRole("button", { name: "Review artist" }),
  );
  const preview = await screen.findByRole("region", {
    name: "Review artist",
  });
  expect(
    within(preview).getByText(artistReviewDetail.artist.value, { exact: true }),
  ).toBeTruthy();
  expect(
    within(preview).getByText(artistPreview.profile.artist.name, {
      exact: true,
    }),
  ).toBeTruthy();
  expect(
    within(preview).getByText(artistPreview.profile.disambiguation, {
      exact: true,
    }),
  ).toBeTruthy();
  expect(JSON.parse(writes()[2][1]?.body as string)).toEqual(artistSelection);
  expect(writes().map(([path]) => path)).toEqual([
    `${review}/context`,
    `${review}/search`,
    `${review}/preview`,
  ]);
  expect(persisted).toBe(false);
  expect(fetch.mock.calls.some(([path]) => path.endsWith("/profile"))).toBe(
    false,
  );
  fireEvent.click(
    within(preview).getByRole("button", {
      name: "Use this artist",
    }),
  );
  await screen.findByText("Artist review saved.", { exact: true });
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
  expect(persisted).toBe(true);
  expect(writes().map(([path]) => path)).toEqual([
    `${review}/context`,
    `${review}/search`,
    `${review}/preview`,
    `${review}/confirm`,
  ]);
  expect(JSON.parse(writes()[3][1]?.body as string)).toEqual(artistSelection);
  expect(fetch.mock.calls.map(([path]) => path)).toHaveLength(10);
  expect(fetch.mock.calls.at(-1)?.[0]).toBe(
    `${associations}/${artistAssociation.id}/profile`,
  );
  for (const [, options] of fetch.mock.calls) {
    expect(options?.credentials).toBe("same-origin");
    expect(new Headers(options?.headers).get("x-hito-account")).toBe(
      artistReviewAccount,
    );
    expect(options?.signal).toBeInstanceOf(AbortSignal);
  }
  for (const privateValue of [
    firstCatalog.installation.id,
    secondCatalog.installation.id,
    firstCatalog.service.base_url,
    secondCatalog.service.base_url,
    ...artistSearchResult.page.items.map((candidate) => candidate.artist.id),
    artistPreview.profile.revision,
  ])
    expect(view.container.textContent).not.toContain(privateValue);
  expect(artistReviewDetail).toEqual(before);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelectorAll("audio")).toHaveLength(0);
  expect(localStorage.length).toBe(0);
  expect(sessionStorage.length).toBe(0);
  view.unmount();
  expect(
    fetch.mock.calls.every(([, options]) => options?.signal?.aborted),
  ).toBe(true);
  expect(errors).not.toHaveBeenCalled();
});
