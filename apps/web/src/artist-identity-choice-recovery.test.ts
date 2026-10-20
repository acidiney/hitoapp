// @vitest-environment jsdom
import { createElement } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistReview from "./components/metadata/ArtistReview";
import type { components } from "./generated";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewConnections,
  artistReviewContext,
  artistReviewDetail,
  artistReviewSettings,
  reviewedArtistProfile,
} from "./fixtures/artistReview";
import {
  artistIdentityGroup,
  artistIdentityGroupDetail,
  selectedArtistIdentity,
  unselectedArtistIdentity,
} from "./fixtures/artistIdentity";

type Schema = components["schemas"];

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  localStorage.clear();
  sessionStorage.clear();
});

test("a rejected stale revision requires an explicit current-choice check before a new selection", async () => {
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  const writes: Schema["ChooseRecordingArtistIdentity"][] = [];
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === "/api/v1/metadata/settings")
      return Response.json(artistReviewSettings);
    if (method === "GET" && path === "/api/v1/plugins")
      return Response.json(artistReviewConnections);
    if (method === "GET" && path === `${base}/artist-associations`)
      return Response.json([artistAssociation]);
    if (
      method === "GET" &&
      path === `${base}/artist-associations/${artistAssociation.id}/profile`
    )
      return Response.json(reviewedArtistProfile);
    if (method === "GET" && path === `${base}/artist-identity`)
      return Response.json(
        writes.length === 0
          ? unselectedArtistIdentity
          : writes.length === 1
            ? { ...unselectedArtistIdentity, revision: 7 }
            : { ...selectedArtistIdentity, revision: 8 },
      );
    if (
      method === "GET" &&
      path === `/api/v1/artist-identity-groups/${artistIdentityGroup}?limit=1`
    )
      return Response.json(artistIdentityGroupDetail);
    if (method === "POST" && path === `${base}/artist-review/context`)
      return Response.json(artistReviewContext);
    if (method === "PUT" && path === `${base}/artist-identity`) {
      const body = JSON.parse(
        options?.body as string,
      ) as Schema["ChooseRecordingArtistIdentity"];
      writes.push(body);
      if (writes.length === 1)
        return Response.json(
          { error: "Owned stale choice", code: "conflict" },
          { status: 409 },
        );
      return Response.json({
        request_id: body.request_id,
        recording_id: artistReviewDetail.recording_id,
        revision: 8,
        selection: {
          association_id: artistAssociation.id,
          group_id: artistIdentityGroup,
        },
      } satisfies Schema["ArtistIdentityChoiceReceipt"]);
    }
    throw new Error(
      "Unexpected request outside the owned artist identity conflict",
    );
  });
  vi.stubGlobal("fetch", fetch);
  render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: artistReviewDetail,
    }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Find artist information" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "View saved artist review 1" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose this reviewed artist" }),
  );
  await screen.findByRole("alert");
  expect(writes).toHaveLength(1);
  expect(
    screen.queryByRole("button", { name: "Choose this reviewed artist" }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", {
      name: "Retry the same artist choice request",
    }),
  ).toBeNull();
  fireEvent.click(
    screen.getByRole("button", { name: "Check current artist choice" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose this reviewed artist" }),
  );
  await screen.findByText("Reviewed artist selected for this track.", {
    exact: true,
  });
  expect(writes).toHaveLength(2);
  expect(writes.map((body) => body.expected_revision)).toEqual([0, 7]);
  expect(writes[1].request_id).not.toBe(writes[0].request_id);
  expect(
    fetch.mock.calls.filter(
      ([path]) => path === `${base}/artist-review/context`,
    ),
  ).toHaveLength(2);
});

test.each(["selected", "needs_review"] as const)(
  "%s choice can be reset explicitly with metadata disabled and an unavailable group label",
  async (status) => {
    const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
    const writes: Schema["ResetRecordingArtistIdentity"][] = [];
    const original = {
      ...selectedArtistIdentity,
      revision: 5,
      state: { ...selectedArtistIdentity.state, status },
    } satisfies Schema["RecordingArtistIdentityChoice"];
    const fetch = vi.fn(async (path: string, options?: RequestInit) => {
      const method = options?.method ?? "GET";
      if (method === "GET" && path === "/api/v1/metadata/settings")
        return Response.json({
          ...artistReviewSettings,
          enabled: false,
          providers: [],
        });
      if (method === "GET" && path === "/api/v1/plugins")
        return Response.json({ services: [], installations: [] });
      if (method === "GET" && path === `${base}/artist-associations`)
        return Response.json([]);
      if (method === "GET" && path === `${base}/artist-identity`)
        return Response.json(
          writes.length
            ? { ...unselectedArtistIdentity, revision: 6 }
            : original,
        );
      if (
        method === "GET" &&
        path === `/api/v1/artist-identity-groups/${artistIdentityGroup}?limit=1`
      )
        return Response.json({ error: "Not found" }, { status: 404 });
      if (method === "POST" && path === `${base}/artist-identity/reset`) {
        const body = JSON.parse(
          options?.body as string,
        ) as Schema["ResetRecordingArtistIdentity"];
        expect(body).toEqual({
          request_id: expect.stringMatching(
            /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
          ),
          expected_revision: 5,
        });
        writes.push(body);
        return Response.json({
          request_id: body.request_id,
          recording_id: artistReviewDetail.recording_id,
          revision: 6,
          selection: null,
        } satisfies Schema["ArtistIdentityChoiceReceipt"]);
      }
      throw new Error("Unexpected request outside local identity reset");
    });
    vi.stubGlobal("fetch", fetch);
    render(
      createElement(ArtistReview, {
        account: artistReviewAccount,
        detail: artistReviewDetail,
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Find artist information" }),
    );
    const reset = await screen.findByRole("button", {
      name: "Reset artist choice for this track",
    });
    expect(writes).toHaveLength(0);
    fireEvent.click(reset);
    await waitFor(() => expect(writes).toHaveLength(1));
    await screen.findByText("No reviewed artist selected for this track.", {
      exact: true,
    });
    expect(
      fetch.mock.calls
        .filter(([, options]) => options?.method === "POST")
        .map(([path]) => path),
    ).toEqual([`${base}/artist-identity/reset`]);
    expect(
      fetch.mock.calls.some(
        ([path]) => path.endsWith("/profile") || path.endsWith("/context"),
      ),
    ).toBe(false);
  },
);

test("an ambiguous choice retries its exact request and displays current state rather than its historical acknowledgement", async () => {
  const warnings = vi.spyOn(console, "warn");
  const errors = vi.spyOn(console, "error");
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  const bodies: string[] = [];
  const current = {
    ...unselectedArtistIdentity,
    revision: 3,
  } satisfies Schema["RecordingArtistIdentityChoice"];
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === "/api/v1/metadata/settings")
      return Response.json(artistReviewSettings);
    if (method === "GET" && path === "/api/v1/plugins")
      return Response.json(artistReviewConnections);
    if (method === "GET" && path === `${base}/artist-associations`)
      return Response.json([artistAssociation]);
    if (
      method === "GET" &&
      path === `${base}/artist-associations/${artistAssociation.id}/profile`
    )
      return Response.json(reviewedArtistProfile);
    if (method === "GET" && path === `${base}/artist-identity`)
      return Response.json(
        bodies.length === 0 ? unselectedArtistIdentity : current,
      );
    if (method === "POST" && path === `${base}/artist-review/context`)
      return Response.json(artistReviewContext);
    if (method === "PUT" && path === `${base}/artist-identity`) {
      bodies.push(options?.body as string);
      const body = JSON.parse(
        bodies.at(-1)!,
      ) as Schema["ChooseRecordingArtistIdentity"];
      if (bodies.length === 1)
        throw new TypeError("Owned private transport detail must stay private");
      return Response.json({
        request_id: body.request_id,
        recording_id: artistReviewDetail.recording_id,
        revision: 1,
        selection: {
          association_id: artistAssociation.id,
          group_id: artistIdentityGroup,
        },
      } satisfies Schema["ArtistIdentityChoiceReceipt"]);
    }
    throw new Error(
      "Unexpected request outside the owned artist identity recovery",
    );
  });
  vi.stubGlobal("fetch", fetch);
  render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: artistReviewDetail,
    }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Find artist information" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "View saved artist review 1" }),
  );
  await screen.findByRole("region", { name: "Reviewed artist profile" });
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose this reviewed artist" }),
  );
  await screen.findByRole("alert");
  expect(bodies).toHaveLength(1);
  expect(
    screen.queryByRole("button", { name: "Choose this reviewed artist" }),
  ).toBeNull();
  fireEvent.click(
    screen.getByRole("button", {
      name: "Retry the same artist choice request",
    }),
  );
  await waitFor(() => expect(bodies).toHaveLength(2));
  await waitFor(() =>
    expect(
      screen.queryByRole("button", {
        name: "Retry the same artist choice request",
      }),
    ).toBeNull(),
  );
  expect(bodies[1]).toBe(bodies[0]);
  expect(
    fetch.mock.calls.filter(
      ([path]) => path === `${base}/artist-review/context`,
    ),
  ).toHaveLength(1);
  expect(
    fetch.mock.calls.filter(
      ([path, options]) =>
        path === `${base}/artist-identity` &&
        (options?.method ?? "GET") === "GET",
    ),
  ).toHaveLength(2);
  expect(
    screen.getByText("No reviewed artist selected for this track.", {
      exact: true,
    }),
  ).toBeTruthy();
  expect(
    screen.queryByText("Reviewed artist selected for this track.", {
      exact: true,
    }),
  ).toBeNull();
  expect(JSON.stringify(warnings.mock.calls)).not.toContain(
    "Owned private transport detail",
  );
  expect(errors).not.toHaveBeenCalled();
});
