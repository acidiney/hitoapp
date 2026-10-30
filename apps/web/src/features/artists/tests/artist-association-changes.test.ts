import { createElement } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ArtistReview from "../components/ArtistReview";
import type { Schema } from "../../../shared/schema";
import { unselectedArtistIdentity } from "../fixtures/artistIdentity";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewConnections,
  artistReviewDetail,
  artistReviewSettings,
} from "../fixtures/artistReview";

function owned(
  options: {
    failFirstUnlink?: boolean;
    startUnlinked?: boolean;
    rejectNext?: number;
  } = {},
) {
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  const associations = `${base}/artist-associations`;
  let linked = !options.startUnlinked;
  let revision = options.startUnlinked ? 2 : 0;
  let rejectNext = options.rejectNext ?? 0;
  let failures = options.failFirstUnlink ? 1 : 0;
  const bodies: Record<string, unknown>[] = [];
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === `${base}/artist-identity`)
      return Response.json(unselectedArtistIdentity);
    if (method === "GET" && path === "/api/v1/metadata/settings")
      return Response.json(artistReviewSettings);
    if (method === "GET" && path === "/api/v1/plugins")
      return Response.json(artistReviewConnections);
    if (method === "GET" && path === associations)
      return Response.json(linked ? [artistAssociation] : []);
    if (method === "GET" && path === `${base}/artist-association-changes`)
      return Response.json({
        recording_id: artistReviewDetail.recording_id,
        revision,
        undo:
          revision === 0
            ? null
            : { association_id: artistAssociation.id, linked: !linked },
        retired: [],
        shared_hidden: 0,
        shareable: false,
      } satisfies Schema<"ArtistAssociationChanges">);
    if (method === "POST" && path.startsWith(`${associations}/`)) {
      const body = JSON.parse(options?.body as string);
      bodies.push(body);
      if (path.endsWith("/unlink") && failures > 0) {
        failures -= 1;
        throw new TypeError("network lost");
      }
      if (rejectNext > 0) {
        rejectNext -= 1;
        revision += 5;
        return Response.json({ code: "conflict" }, { status: 409 });
      }
      expect(body.expected_revision).toBe(revision);
      revision += 1;
      linked = path.endsWith("/undo") ? !linked : false;
      return Response.json({
        request_id: body.request_id,
        recording_id: artistReviewDetail.recording_id,
        revision,
        association_id: artistAssociation.id,
        linked,
      } satisfies Schema<"ArtistAssociationChangeReceipt">);
    }
    throw new Error("Unexpected HTTP request outside the association changes");
  });
  vi.stubGlobal("fetch", fetch);
  return { fetch, bodies, associations };
}

async function open() {
  render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: artistReviewDetail,
    }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Find artist information" }),
  );
  const saved = await screen.findByRole("region", {
    name: "Saved artist reviews",
  });
  await waitFor(() =>
    expect(
      within(saved)
        .getAllByRole("button", { name: /Unlink saved artist review/ })
        .every((button) => !(button as HTMLButtonElement).disabled),
    ).toBe(true),
  );
  return saved;
}

test("unlinking a saved artist review removes it and undo restores it without provider requests", async () => {
  const { fetch, bodies, associations } = owned();
  const saved = await open();
  expect(
    within(saved).getByRole("button", { name: "View saved artist review 1" }),
  ).toBeTruthy();
  expect(screen.queryByRole("button", { name: /Undo last/ })).toBeNull();

  fireEvent.click(
    within(saved).getByRole("button", { name: "Unlink saved artist review 1" }),
  );
  const undo = await screen.findByRole("button", {
    name: "Undo last artist review change",
  });
  expect(bodies).toHaveLength(1);
  expect(bodies[0]).toEqual({
    request_id: expect.any(String),
    expected_revision: 0,
    association_id: artistAssociation.id,
  } satisfies Schema<"UnlinkRecordingArtist"> & Record<string, unknown>);
  expect(
    screen.queryByRole("button", { name: /View saved artist review/ }),
  ).toBeNull();

  fireEvent.click(undo);
  await screen.findByRole("button", { name: "View saved artist review 1" });
  expect(bodies[1]).toEqual({
    request_id: expect.any(String),
    expected_revision: 1,
  } satisfies Schema<"UndoRecordingArtistChange">);
  expect(bodies[1].request_id).not.toBe(bodies[0].request_id);
  expect(
    fetch.mock.calls.filter(([path]) => path.endsWith("/profile")),
  ).toHaveLength(0);
  expect(
    fetch.mock.calls
      .filter(([, options]) => options?.method === "POST")
      .map(([path]) => path),
  ).toEqual([`${associations}/unlink`, `${associations}/undo`]);
});

test("a lost unlink acknowledgement is recovered by retrying the exact same request", async () => {
  const { bodies } = owned({ failFirstUnlink: true });
  const saved = await open();
  fireEvent.click(
    within(saved).getByRole("button", { name: "Unlink saved artist review 1" }),
  );
  const retry = await screen.findByRole("button", {
    name: "Retry the same artist review change request",
  });
  expect(screen.getByRole("alert")).toBeTruthy();
  fireEvent.click(retry);
  await waitFor(() =>
    expect(
      screen.queryByRole("button", { name: /View saved artist review/ }),
    ).toBeNull(),
  );
  expect(bodies).toHaveLength(2);
  expect(bodies[1]).toEqual(bodies[0]);
});

test("a stale undo is rejected, the server state is reread and the next undo works", async () => {
  const { bodies } = owned({ startUnlinked: true, rejectNext: 1 });
  render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: artistReviewDetail,
    }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Find artist information" }),
  );
  const undo = await screen.findByRole("button", {
    name: "Undo last artist review change",
  });
  fireEvent.click(undo);
  await screen.findByRole("alert");
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Undo last artist review change",
    }),
  );
  await screen.findByRole("button", { name: "View saved artist review 1" });
  expect(bodies.map((body) => body.expected_revision)).toEqual([2, 7]);
});

test("an administrator can share the latest built-in unlink and sees retired unlinks", async () => {
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  const bodies: { path: string; body: Record<string, unknown> }[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      const method = options?.method ?? "GET";
      if (method === "GET" && path === `${base}/artist-identity`)
        return Response.json(unselectedArtistIdentity);
      if (method === "GET" && path === "/api/v1/metadata/settings")
        return Response.json(artistReviewSettings);
      if (method === "GET" && path === "/api/v1/plugins")
        return Response.json(artistReviewConnections);
      if (method === "GET" && path === `${base}/artist-associations`)
        return Response.json([]);
      if (method === "GET" && path === `${base}/artist-association-changes`)
        return Response.json({
          recording_id: artistReviewDetail.recording_id,
          revision: 2,
          undo: { association_id: artistAssociation.id, linked: true },
          retired: [
            { association_id: "older-association", reason: "credit_changed" },
          ],
          shared_hidden: 2,
          shareable: bodies.length === 0,
          shared: bodies.length ? { id: "shared-unlink", revision: 1 } : null,
        } satisfies Schema<"ArtistAssociationChanges">);
      if (method === "GET" && path.startsWith("/api/v1/artist-corrections?"))
        return Response.json({
          revision: 4,
          items: [],
          total: 0,
          offset: 0,
          limit: 25,
        } satisfies Schema<"SharedArtistCorrections">);
      if (method === "POST") {
        const body = JSON.parse(options?.body as string);
        bodies.push({ path, body });
        return Response.json({
          request_id: body.request_id,
          correction_id: "shared-unlink",
          revision: 1,
          applied: true,
          recordings: 1,
        } satisfies Schema<"SharedArtistCorrectionReceipt">);
      }
      throw new Error(`Unexpected request ${method} ${path}`);
    }),
  );
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
    await screen.findByRole("button", {
      name: "Apply this unlink for everyone",
    }),
  );
  await screen.findByRole("button", { name: "Withdraw the shared unlink" });
  expect(bodies).toEqual([
    {
      path: "/api/v1/artist-corrections/promote",
      body: {
        request_id: expect.any(String),
        expected_revision: 4,
        source: {
          kind: "unlink",
          recording_id: artistReviewDetail.recording_id,
          association_id: artistAssociation.id,
        },
      },
    },
  ]);
  expect(screen.getByText(/1 unlink no longer applies/)).toBeTruthy();
  expect(
    screen.getByText(
      /2 saved reviews are hidden by a correction an administrator shared/,
    ),
  ).toBeTruthy();
});
