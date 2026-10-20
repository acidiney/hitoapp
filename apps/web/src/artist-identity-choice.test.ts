// @vitest-environment jsdom
import { createElement } from "react";
import {
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
import {
  artistIdentityGroup,
  artistIdentityGroupDetail,
  selectedArtistIdentity,
  unselectedArtistIdentity,
} from "./fixtures/artistIdentity";
import {
  artistAssociation,
  artistPreview,
  artistReviewAccount,
  artistReviewConnections,
  artistReviewContext,
  artistReviewDetail,
  artistReviewSettings,
  artistSearchResult,
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

test("saving an artist review leaves identity unselected until its reviewed association is explicitly chosen", async () => {
  const errors = vi.spyOn(console, "error");
  const before = structuredClone(artistReviewDetail);
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  const associationPath = `${base}/artist-associations`;
  const choicePath = `${base}/artist-identity`;
  let confirmed = false;
  let selected = false;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === "/api/v1/metadata/settings")
      return Response.json(artistReviewSettings);
    if (method === "GET" && path === "/api/v1/plugins")
      return Response.json(artistReviewConnections);
    if (method === "GET" && path === associationPath)
      return Response.json(confirmed ? [artistAssociation] : []);
    if (method === "POST" && path === `${base}/artist-review/context`)
      return Response.json(artistReviewContext);
    if (method === "POST" && path === `${base}/artist-review/search`)
      return Response.json(artistSearchResult);
    if (method === "POST" && path === `${base}/artist-review/preview`)
      return Response.json(artistPreview);
    if (method === "POST" && path === `${base}/artist-review/confirm`) {
      confirmed = true;
      return Response.json(artistAssociation);
    }
    if (
      method === "GET" &&
      path === `${associationPath}/${artistAssociation.id}/profile`
    )
      return Response.json(reviewedArtistProfile);
    if (method === "GET" && path === choicePath)
      return Response.json(
        selected ? selectedArtistIdentity : unselectedArtistIdentity,
      );
    if (
      method === "GET" &&
      path === `/api/v1/artist-identity-groups/${artistIdentityGroup}?limit=1`
    )
      return Response.json(artistIdentityGroupDetail);
    if (method === "PUT" && path === choicePath) {
      expect(confirmed).toBe(true);
      const body = JSON.parse(options?.body as string);
      expect(body).toEqual({
        request_id: expect.stringMatching(
          /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
        ),
        expected_revision: 0,
        association_id: artistAssociation.id,
        context: artistReviewContext,
      });
      selected = true;
      return Response.json({
        request_id: body.request_id,
        recording_id: artistReviewDetail.recording_id,
        revision: 1,
        selection: {
          group_id: artistIdentityGroup,
          association_id: artistAssociation.id,
        },
      } satisfies components["schemas"]["ArtistIdentityChoiceReceipt"]);
    }
    throw new Error(
      "Unexpected request outside the owned artist identity journey",
    );
  });
  vi.stubGlobal("fetch", fetch);
  render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: artistReviewDetail,
    }),
  );
  expect(fetch).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", { name: "Find artist information" }),
  );
  fireEvent.change(
    await screen.findByRole("combobox", { name: "Metadata provider" }),
    {
      target: { value: secondCatalog.installation.id },
    },
  );
  fireEvent.click(screen.getByRole("button", { name: "Search artists" }));
  const candidates = await screen.findByRole("list", {
    name: "Artist search results",
  });
  fireEvent.click(
    within(within(candidates).getAllByRole("listitem")[1]).getByRole("button", {
      name: "Review artist",
    }),
  );
  const preview = await screen.findByRole("region", { name: "Review artist" });
  fireEvent.click(
    within(preview).getByRole("button", { name: "Use this artist" }),
  );
  await screen.findByRole("region", { name: "Reviewed artist profile" });
  expect(confirmed).toBe(true);
  expect(selected).toBe(false);
  expect(
    fetch.mock.calls.filter(([, options]) => options?.method === "PUT"),
  ).toHaveLength(0);

  fireEvent.click(
    screen.getByRole("button", { name: "Choose this reviewed artist" }),
  );
  await waitFor(() => expect(selected).toBe(true));
  await screen.findByText("Reviewed artist selected for this track.", {
    exact: true,
  });
  await screen.findByText(
    `Saved credit for this group: ${artistIdentityGroupDetail.group.name.value}`,
    { exact: true },
  );
  const writes = fetch.mock.calls.filter(
    ([, options]) => options?.method === "PUT",
  );
  expect(writes).toHaveLength(1);
  const writeIndex = fetch.mock.calls.indexOf(writes[0]);
  expect(
    fetch.mock.calls
      .slice(writeIndex + 1)
      .some(
        ([path, options]) =>
          path === choicePath && (options?.method ?? "GET") === "GET",
      ),
  ).toBe(true);
  const contexts = fetch.mock.calls.filter(
    ([path]) => path === `${base}/artist-review/context`,
  );
  expect(contexts).toHaveLength(2);
  expect(JSON.parse(contexts[1][1]?.body as string)).toEqual({
    provider: artistAssociation.provider,
  });
  for (const [, options] of fetch.mock.calls) {
    expect(options?.credentials).toBe("same-origin");
    expect(new Headers(options?.headers).get("x-hito-account")).toBe(
      artistReviewAccount,
    );
    expect(options?.signal).toBeInstanceOf(AbortSignal);
  }
  expect(artistReviewDetail).toEqual(before);
  expect(errors).not.toHaveBeenCalled();
});
